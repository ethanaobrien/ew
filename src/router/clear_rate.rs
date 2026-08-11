use jzon::{array, object, JsonValue};
use actix_web::{http::header::ContentType, HttpRequest, HttpResponse, Responder};
use rusqlite::params;
use std::sync::Mutex;
use lazy_static::lazy_static;

use crate::sql::SQLite;
use crate::router::{databases, global, userdata, Session, Api};
use crate::include_file;
use crate::router::tools::guest;

trait SqlClearRate {
    fn get_live_data(&self, id: i64) -> Result<Live, rusqlite::Error>;
}
impl SqlClearRate for SQLite {
    fn get_live_data(&self, id: i64) -> Result<Live, rusqlite::Error> {
        let conn = rusqlite::Connection::open(self.get_path()).unwrap();
        let mut stmt = conn.prepare("SELECT * FROM lives WHERE live_id=?1")?;
        stmt.query_row(params!(id), |row| {
            Ok(Live {
               live_id: row.get(0)?,
               normal_failed: row.get(1)?,
               normal_pass: row.get(2)?,
               hard_failed: row.get(3)?,
               hard_pass: row.get(4)?,
               expert_failed: row.get(5)?,
               expert_pass: row.get(6)?,
               master_failed: row.get(7)?,
               master_pass: row.get(8)?,
            })
        })
    }
}

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("live_statistics.db", setup_tables);
    static ref CACHED_DATA: Mutex<Option<JsonValue>> = Mutex::new(None);
    static ref CACHED_HTML_DATA: Mutex<Option<JsonValue>> = Mutex::new(None);
}

pub struct Live {
    pub live_id: i32,
    pub normal_failed: i64,
    pub normal_pass: i64,
    pub hard_failed: i64,
    pub hard_pass: i64,
    pub expert_failed: i64,
    pub expert_pass: i64,
    pub master_failed: i64,
    pub master_pass: i64,
}

fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS lives (
        live_id         INT NOT NULL PRIMARY KEY,
        normal_failed   BIGINT NOT NULL,
        normal_pass     BIGINT NOT NULL,
        hard_failed     BIGINT NOT NULL,
        hard_pass       BIGINT NOT NULL,
        expert_failed   BIGINT NOT NULL,
        expert_pass     BIGINT NOT NULL,
        master_failed   BIGINT NOT NULL,
        master_pass     BIGINT NOT NULL
    );
    CREATE TABLE IF NOT EXISTS scores (
        live_id      INT NOT NULL PRIMARY KEY,
        score_data   TEXT NOT NULL
    );").unwrap();
}

// Merges this play into the song's top-10 board. Pure so the whole read-modify-write can
// sit inside one transaction, and so the keep-best rule is testable on its own.
//
// The board is per-song, not per-account: `scores` is keyed by live_id alone and the user
// lives inside the JSON blob. A user already on the board keeps whichever of their two
// scores is higher — a replay that does not beat the stored one is dropped (`None`), which
// is what makes a repeated or duplicated end idempotent rather than additive.
fn merge_live_score(scores: &JsonValue, uid: i64, score: i64) -> Option<JsonValue> {
    let mut result = array![];
    let mut current = 0;
    let mut added = false;
    for i in 0..10 {
        if current >= 10 {
            break;
        }
        if scores[i].is_empty() && !added {
            added = true;
            result.push(object!{user: uid, score: score}).unwrap();
        }
        if scores[i].is_empty() {
            break;
        }
        if scores[i]["score"].as_i64().unwrap() < score && !added {
            added = true;
            result.push(object!{user: uid, score: score}).unwrap();
            current += 1;
            if current >= 10 {
                break;
            }
        }
        if scores[i]["user"].as_i64().unwrap() == uid && !added {
            // Already on the board with a better score — keep it, drop this play.
            return None;
        }
        if scores[i]["user"].as_i64().unwrap() == uid {
            continue;
        }
        result.push(scores[i].clone()).unwrap();
        current += 1;
    }

    if added { Some(result) } else { None }
}

fn update_live_score(id: i64, uid: i64, score: i64) {
    if uid == 0 || score == 0 {
        return;
    }

    // One transaction for read + merge + write. Previously this SELECTed to choose between
    // UPDATE and INSERT on separate connections, so two ends landing together for a song
    // with no row yet both took the INSERT branch and the loser panicked the worker on
    // `UNIQUE constraint failed: scores.live_id`. Two clients finishing the same multi
    // live make that the normal case, not a rare race.
    let write = DATABASE.lock_and_transact(|conn| {
        let stored: String = conn
            .query_row("SELECT score_data FROM scores WHERE live_id=?1", params!(id), |row| row.get(0))
            .unwrap_or_else(|_| String::from("[]"));
        let scores = jzon::parse(&stored).unwrap_or_else(|_| array![]);

        let Some(result) = merge_live_score(&scores, uid, score) else {
            return Ok(());
        };

        // Atomic upsert: no branch left for a concurrent writer to slip between.
        conn.execute(
            "INSERT INTO scores (live_id, score_data) VALUES (?1, ?2)
             ON CONFLICT(live_id) DO UPDATE SET score_data=excluded.score_data",
            params!(id, jzon::stringify(result))
        )?;
        Ok(())
    });

    if let Err(e) = write {
        println!("Failed to record score for live {id}: {e}");
    }
}

// Delete live id when custom song deleted
pub fn purge_live(live_id: i64) {
    DATABASE.lock_and_exec("DELETE FROM lives WHERE live_id=?1", params!(live_id));
    DATABASE.lock_and_exec("DELETE FROM scores WHERE live_id=?1", params!(live_id));
    invalidate_cache();
}

pub fn invalidate_cache() {
    crate::lock_onto_mutex!(CACHED_DATA).take();
    crate::lock_onto_mutex!(CACHED_HTML_DATA).take();
}

// The clear-rate counter column this play lands in, or None for a level outside 1-4.
// Names come from this closed set, never from request data, so it is safe to interpolate.
fn clear_rate_column(level: i32, failed: bool) -> Option<&'static str> {
    let tier = match level {
        1 => "normal",
        2 => "hard",
        3 => "expert",
        4 => "master",
        _ => return None
    };
    Some(match (tier, failed) {
        ("normal", true) => "normal_failed",   ("normal", false) => "normal_pass",
        ("hard", true) => "hard_failed",       ("hard", false) => "hard_pass",
        ("expert", true) => "expert_failed",   ("expert", false) => "expert_pass",
        (_, true) => "master_failed",          (_, false) => "master_pass"
    })
}

pub fn live_completed(id: i64, level: i32, failed: bool, score: i64, uid: i64) {
    update_live_score(id, uid, score);

    let Some(column) = clear_rate_column(level, failed) else {
        return;
    };

    // `lives` is keyed by live_id alone and had the same select-then-INSERT-or-UPDATE
    // split as `scores`, so it could panic the same way on a first-ever concurrent play
    // (and lost counts whenever two plays overlapped). One upsert does both branches
    // atomically and increments in SQL rather than read-modify-write in Rust.
    let write = DATABASE.lock_and_transact(|conn| {
        conn.execute(
            &format!(
                "INSERT INTO lives (live_id, normal_failed, normal_pass, hard_failed, hard_pass,
                                    expert_failed, expert_pass, master_failed, master_pass)
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)
                 ON CONFLICT(live_id) DO UPDATE SET {column} = {column} + 1"
            ),
            params!(
                id,
                if 1 == level && failed { 1 } else { 0 },
                if 1 == level && !failed { 1 } else { 0 },
                if 2 == level && failed { 1 } else { 0 },
                if 2 == level && !failed { 1 } else { 0 },
                if 3 == level && failed { 1 } else { 0 },
                if 3 == level && !failed { 1 } else { 0 },
                if 4 == level && failed { 1 } else { 0 },
                if 4 == level && !failed { 1 } else { 0 }
            )
        )?;
        Ok(())
    });

    if let Err(e) = write {
        println!("Failed to record clear rate for live {id}: {e}");
    }
}

fn get_song_title(live_id: i32, english: bool) -> String {
    let details = if english {
        databases::MUSIC_EN[live_id.to_string()].clone()
    } else {
        databases::MUSIC[live_id.to_string()].clone()
    };
    if !details.is_null() {
        return details["name"].to_string();
    }
    // Custom songs aren't in the official music mst (their live_id ==
    // music_id). PUBLIC ones show their real title; private/shared ones are
    // already filtered out of the page and would stay "Unknown Song" even if
    // one slipped through - the lookup only ever answers for public songs
    if let Some(title) = crate::router::custom_song::public_song_title(live_id as i64, english) {
        return title;
    }
    String::from("Unknown Song")
}

// Titles land inside HTML text and attributes; custom song names are user
// input, so escape them (official names contain nothing that needs it)
fn html_escape(text: &str) -> String {
    text.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn get_pass_percent(failed: i64, pass: i64) -> String {
    let total = (failed + pass) as f64;
    if failed + pass == 0 {
        return String::from("--:--%");
    }
    let pass = pass as f64;
    format!("{:.2}%", pass / total * 100.0)
}

fn get_json() -> JsonValue {
    let lives = DATABASE.lock_and_select_all("SELECT live_id FROM lives", params!()).unwrap();
    let mut rates = array![];
    let mut ids = array![];
    for id in lives.members() {
        let info = DATABASE.get_live_data(id.as_i64().unwrap());
        if info.is_err() {
            continue;
        }
        let info = info.unwrap();
        let to_push = object!{
            master_live_id: info.live_id,
            normal: get_pass_percent(info.normal_failed, info.normal_pass),
            hard: get_pass_percent(info.hard_failed, info.hard_pass),
            expert: get_pass_percent(info.expert_failed, info.expert_pass),
            master: get_pass_percent(info.master_failed, info.master_pass)
        };
        // Custom songs aren't in the official live mst; their live_id == music_id
        ids.push(databases::LIVE_LIST[info.live_id.to_string()]["masterMusicId"].as_i64().unwrap_or(info.live_id as i64)).unwrap();
        rates.push(to_push).unwrap();
    }
    object!{
        "cache": {
            "all_user_clear_rate": rates,
            "master_music_ids": ids,
            "event_live_list": []
        },
        "last_updated": global::timestamp()
    }
}

async fn get_clearrate_json() -> JsonValue {
    let cache = {
        let mut result = crate::lock_onto_mutex!(CACHED_DATA);
        if result.is_none() {
            result.replace(get_json());
        }
        result.as_ref().unwrap().clone()
    };
    let rv = cache["cache"].clone();
    if cache["last_updated"].as_u64().unwrap() + (60 * 60) < global::timestamp() {
        let mut result = crate::lock_onto_mutex!(CACHED_DATA);
        let new = get_json();
        result.replace(new.clone());
    }
    rv
}

pub async fn clearrate(req: HttpRequest) -> impl Responder {
    let mut data = get_clearrate_json().await;
    let hidden = crate::router::custom_song::hidden_live_ids_for_user(global::get_uid(req.headers()));
    if !hidden.is_empty() {
        let rates = data["all_user_clear_rate"].clone();
        let ids = data["master_music_ids"].clone();
        let mut new_rates = array![];
        let mut new_ids = array![];
        for (i, rate) in rates.members().enumerate() {
            if hidden.contains(rate["master_live_id"].as_i64().unwrap()) {
                continue;
            }
            new_rates.push(rate.clone()).unwrap();
            new_ids.push(ids[i].clone()).unwrap();
        }
        data["all_user_clear_rate"] = new_rates;
        data["master_music_ids"] = new_ids;
    }
    Api(Some(data))
}

pub async fn ranking(req: HttpRequest, Session { key, body }: Session) -> impl Responder {
    let protocol = crate::router::global::client_protocol_version(&req);
    let self_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let live = body["master_live_id"].as_i64().unwrap();

    let info = DATABASE.lock_and_select("SELECT score_data FROM scores WHERE live_id=?1", params!(live)).unwrap_or(String::from("[]"));
    let scores = jzon::parse(&info).unwrap();

    let mut rank = array![];

    for (i, data) in scores.members().enumerate() {
        let uid = data["user"].as_i64().unwrap();
        let user = guest::get_user(uid, &object![], guest::UserView::Ranking, protocol);
        let user_obj = if uid == self_id {
            // The client wants the fields get_user hides from other players
            let mut self_user = object!{
                user: userdata::get_acc_from_uid(uid)["user"].clone()
            };
            guest::proxy_user_cards(&mut self_user, protocol);
            self_user["user"].clone()
        } else {
            user["user"].clone()
        };
        rank.push(object!{
            rank: i + 1,
            user: user_obj,
            score: data["score"].as_i64().unwrap(),
            favorite_card: user["favorite_card"].clone(),
            guest_smile_card: user["guest_smile_card"].clone(),
            guest_cool_card: user["guest_cool_card"].clone(),
            guest_pure_card: user["guest_pure_card"].clone()
        }).unwrap();
    }

    Api(Some(object!{
        "ranking_list": rank
    }))
}

fn get_html() -> JsonValue {
    let lives = DATABASE.lock_and_select_all("SELECT live_id FROM lives", params!()).unwrap();
    let hidden = crate::router::custom_song::hidden_live_ids();

    let mut table = String::new();

    for id in lives.members() {
        let live_id = id.as_i64().unwrap();
        if hidden.contains(live_id) {
            continue;
        }

        let info = match DATABASE.get_live_data(live_id) {
            Ok(i) => i,
            Err(_) => continue,
        };

        let calc_rate = |pass: i64, fail: i64| -> f64 {
            let total = pass + fail;
            if total == 0 { 0.0 } else { pass as f64 / total as f64 }
        };

        let title_jp = html_escape(&get_song_title(info.live_id, false));
        let title_en = html_escape(&get_song_title(info.live_id, true));

        let normal_txt = get_pass_percent(info.normal_failed, info.normal_pass);
        let hard_txt = get_pass_percent(info.hard_failed, info.hard_pass);
        let expert_txt = get_pass_percent(info.expert_failed, info.expert_pass);
        let master_txt = get_pass_percent(info.master_failed, info.master_pass);

        let normal_plays = info.normal_pass + info.normal_failed;
        let hard_plays = info.hard_pass + info.hard_failed;
        let expert_plays = info.expert_pass + info.expert_failed;
        let master_plays = info.master_pass + info.master_failed;

        let normal_rate_sort = calc_rate(info.normal_pass, info.normal_failed);
        let hard_rate_sort = calc_rate(info.hard_pass, info.hard_failed);
        let expert_rate_sort = calc_rate(info.expert_pass, info.expert_failed);
        let master_rate_sort = calc_rate(info.master_pass, info.master_failed);

        table.push_str(&format!(
            r#"<tr>
                <td class="title-cell"
                    data-val="{title_jp}"
                    data-title-en="{title_en}"
                    data-title-jp="{title_jp}">
                    {title_jp}
                </td>

                <td data-plays="{normal_plays}" data-rate="{normal_rate_sort}">
                    <span class="rate-text">{normal_txt}</span>
                    <span class="meta-text">{normal_plays} plays</span>
                </td>

                <td data-plays="{hard_plays}" data-rate="{hard_rate_sort}">
                    <span class="rate-text">{hard_txt}</span>
                    <span class="meta-text">{hard_plays} plays</span>
                </td>

                <td data-plays="{expert_plays}" data-rate="{expert_rate_sort}">
                    <span class="rate-text">{expert_txt}</span>
                    <span class="meta-text">{expert_plays} plays</span>
                </td>

                <td data-plays="{master_plays}" data-rate="{master_rate_sort}">
                    <span class="rate-text">{master_txt}</span>
                    <span class="meta-text">{master_plays} plays</span>
                </td>
            </tr>"#
        ));
    }

    let html = include_file!("src/router/clear_rate_template.html").replace("{{TABLEBODY}}", &table);
    object!{
        "cache": html,
        "last_updated": global::timestamp()
    }
}

async fn get_clearrate_html() -> String {
    let cache = {
        let mut result = crate::lock_onto_mutex!(CACHED_HTML_DATA);
        if result.is_none() {
            result.replace(get_html());
        }
        result.as_ref().unwrap().clone()
    };
    if cache["last_updated"].as_u64().unwrap() + (60 * 60) < global::timestamp() {
        let mut result = crate::lock_onto_mutex!(CACHED_HTML_DATA);
        result.replace(get_html());
    }
    cache["cache"].to_string()
}

pub async fn clearrate_html(_req: HttpRequest) -> HttpResponse {
    let html = get_clearrate_html().await;

    HttpResponse::Ok()
        .content_type(ContentType::html())
        .body(html)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(live_id: i64) -> JsonValue {
        let stored = DATABASE
            .lock_and_select("SELECT score_data FROM scores WHERE live_id=?1", params!(live_id))
            .unwrap_or_else(|_| String::from("[]"));
        jzon::parse(&stored).unwrap()
    }

    fn passes(live_id: i64) -> i64 {
        DATABASE.get_live_data(live_id).map(|l| l.master_pass).unwrap_or(0)
    }

    #[test]
    fn merge_keeps_the_users_best_score() {
        let existing = array![object!{user: 7, score: 900}];

        // A better score replaces the stored one rather than adding a second entry.
        let better = merge_live_score(&existing, 7, 1000).expect("a better score is recorded");
        assert_eq!(better.len(), 1);
        assert_eq!(better[0]["score"].as_i64(), Some(1000));

        // A worse or equal replay is dropped entirely — this is what makes a repeated
        // end idempotent instead of appending the same user twice.
        assert!(merge_live_score(&existing, 7, 800).is_none());
        assert!(merge_live_score(&existing, 7, 900).is_none());

        // A different user is ranked against the board, best first.
        let other = merge_live_score(&existing, 8, 950).expect("a new user is recorded");
        assert_eq!(other.len(), 2);
        assert_eq!(other[0]["user"].as_i64(), Some(8));
        assert_eq!(other[1]["user"].as_i64(), Some(7));
    }

    #[test]
    fn a_duplicate_end_does_not_double_the_board_entry() {
        let _lock = crate::runtime::lock_test_data_path();
        let live_id = 990001;

        live_completed(live_id, 4, false, 500000, 4242);
        assert_eq!(board(live_id).len(), 1);
        assert_eq!(passes(live_id), 1);

        // The second end for the same session: same user, same score. The board must not
        // grow, and this must not raise UNIQUE constraint failed: scores.live_id.
        live_completed(live_id, 4, false, 500000, 4242);
        assert_eq!(board(live_id).len(), 1, "the same user must not appear twice");
        assert_eq!(board(live_id)[0]["score"].as_i64(), Some(500000));
    }

    // The actual regression: two ends for a song with no row yet, landing together. Both
    // used to take the INSERT branch and the loser unwrapped a ConstraintViolation into a
    // worker panic. Two clients finishing one multi live makes this the normal case.
    #[test]
    fn concurrent_first_plays_of_one_song_do_not_collide() {
        let _lock = crate::runtime::lock_test_data_path();
        let live_id = 990002;

        std::thread::scope(|s| {
            for uid in [101i64, 102, 103, 104, 105, 106, 107, 108] {
                s.spawn(move || live_completed(live_id, 4, false, 400000 + uid, uid));
            }
        });

        // Every writer landed: no row lost to a race, none lost to a swallowed error.
        assert_eq!(board(live_id).len(), 8);
        assert_eq!(passes(live_id), 8, "clear-rate counts must not be lost either");
    }

    // The other half of /multi_live/end's score-board branch (the account's own high score
    // is pinned in live.rs): a public multi live reaches live_completed with uid 0, so the
    // play is counted and the board is left alone. /live/retire has always used the same
    // signal for a failed live.
    #[test]
    fn a_play_with_no_user_counts_the_clear_but_not_the_board() {
        let _lock = crate::runtime::lock_test_data_path();
        let live_id = 990003;

        live_completed(live_id, 4, false, 500000, 0);
        assert_eq!(board(live_id).len(), 0, "an unranked play must not reach the board");
        assert_eq!(passes(live_id), 1, "but it is still a play of the song");

        // The same score from a real account does land, which is the private-room path.
        live_completed(live_id, 4, false, 500000, 4243);
        assert_eq!(board(live_id).len(), 1);
        assert_eq!(passes(live_id), 2);
    }

    #[test]
    fn clear_rate_columns_cover_every_level() {
        assert_eq!(clear_rate_column(1, false), Some("normal_pass"));
        assert_eq!(clear_rate_column(1, true), Some("normal_failed"));
        assert_eq!(clear_rate_column(2, false), Some("hard_pass"));
        assert_eq!(clear_rate_column(3, true), Some("expert_failed"));
        assert_eq!(clear_rate_column(4, false), Some("master_pass"));
        assert_eq!(clear_rate_column(4, true), Some("master_failed"));
        // Level 0 (a skip ticket's "any level") writes no counter, as before.
        assert_eq!(clear_rate_column(0, false), None);
        assert_eq!(clear_rate_column(5, false), None);
    }
}

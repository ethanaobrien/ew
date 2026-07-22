use jzon::{array, object, JsonValue};
use actix_web::{http::header::ContentType, HttpRequest, HttpResponse, Responder};
use rusqlite::params;
use std::sync::Mutex;
use lazy_static::lazy_static;

use crate::encryption;
use crate::sql::SQLite;
use crate::router::{databases, global, userdata};
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

fn update_live_score(id: i64, uid: i64, score: i64) {
    if uid == 0 || score == 0 {
        return;
    }
    
    let info = DATABASE.lock_and_select("SELECT score_data FROM scores WHERE live_id=?1", params!(id)).unwrap_or(String::from("[]"));
    let scores = jzon::parse(&info).unwrap();
    
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
            return;
        }
        if scores[i]["user"].as_i64().unwrap() == uid {
            continue;
        }
        result.push(scores[i].clone()).unwrap();
        current += 1;
    }
    
    if added {
        if DATABASE.lock_and_select("SELECT live_id FROM scores WHERE live_id=?1", params!(id)).is_ok() {
            DATABASE.lock_and_exec("UPDATE scores SET score_data=?1 WHERE live_id=?2", params!(jzon::stringify(result), id));
        } else {
            DATABASE.lock_and_exec("INSERT INTO scores (score_data, live_id) VALUES (?1, ?2)", params!(jzon::stringify(result), id));
        }
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

pub fn live_completed(id: i64, level: i32, failed: bool, score: i64, uid: i64) {
    update_live_score(id, uid, score);
    match DATABASE.get_live_data(id) {
        Ok(info) => {
            let value = format!("{}_{}", 
                if 1 == level { "normal" } else if 2 == level { "hard" } else if 3 == level { "expert" } else { "master" },
                if failed { "failed" } else { "pass" }
            );
            let new_info = if 1 == level && failed { info.normal_failed }
                           else if 1 == level && !failed { info.normal_pass }
                           else if 2 == level && failed { info.hard_failed }
                           else if 2 == level && !failed { info.hard_pass }
                           else if 3 == level && failed { info.expert_failed }
                           else if 3 == level && !failed { info.expert_pass }
                           else if 4 == level && failed { info.master_failed }
                           else if 4 == level && !failed { info.master_pass } else { return; };
            
            DATABASE.lock_and_exec(&format!("UPDATE lives SET {}=?1 WHERE live_id=?2", value), params!(new_info + 1, info.live_id));
        },
        Err(_) => {
            DATABASE.lock_and_exec("INSERT INTO lives (live_id, normal_failed, normal_pass, hard_failed, hard_pass, expert_failed, expert_pass, master_failed, master_pass) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9)", params!(
                id,
                if 1 == level && failed { 1 } else { 0 },
                if 1 == level && !failed { 1 } else { 0 },
                if 2 == level && failed { 1 } else { 0 },
                if 2 == level && !failed { 1 } else { 0 },
                if 3 == level && failed { 1 } else { 0 },
                if 3 == level && !failed { 1 } else { 0 },
                if 4 == level && failed { 1 } else { 0 },
                if 4 == level && !failed { 1 } else { 0 }
            ));
        },
    };
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
    String::from("Unknown Song")
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
    global::api(&req, Some(data))
}

pub async fn ranking(req: HttpRequest, body: String) -> impl Responder {
    let key = global::get_login(req.headers(), &body);
    let body = jzon::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let self_id = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let live = body["master_live_id"].as_i64().unwrap();

    let info = DATABASE.lock_and_select("SELECT score_data FROM scores WHERE live_id=?1", params!(live)).unwrap_or(String::from("[]"));
    let scores = jzon::parse(&info).unwrap();

    let mut rank = array![];

    for (i, data) in scores.members().enumerate() {
        let uid = data["user"].as_i64().unwrap();
        let user = guest::get_user(uid, &object![], guest::UserView::Ranking);
        let user_obj = if uid == self_id {
            userdata::get_acc_from_uid(uid)["user"].clone()
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

    global::api(&req, Some(object!{
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

        let title_jp = get_song_title(info.live_id, false);
        let title_en = get_song_title(info.live_id, true);

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

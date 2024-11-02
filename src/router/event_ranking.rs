use json::{object, array, JsonValue};
use rusqlite::params;
use std::sync::Mutex;
use lazy_static::lazy_static;

use crate::sql::SQLite;
use crate::router::global;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("event_ranking.db", setup_tables);
    static ref CACHED_DATA: Mutex<Option<JsonValue>> = Mutex::new(None);
}

fn setup_tables(conn: &SQLite) {
    conn.lock_and_exec("CREATE TABLE IF NOT EXISTS scores (
        event_id     INT NOT NULL PRIMARY KEY,
        score_data   TEXT NOT NULL
    )", params!());
}

pub fn live_completed(event_id: u32, uid: i64, score: i64, star_level: i64) {
    if uid == 0 || score == 0 {
        return;
    }

    let info = DATABASE.lock_and_select("SELECT score_data FROM scores WHERE event_id=?1", params!(event_id)).unwrap_or(String::from("[]"));
    let scores = json::parse(&info).unwrap();

    let mut result = array![];
    let mut current = 0;
    let mut added = false;
    for i in 0..10000 {
        if current >= 10000 {
            break;
        }
        if scores[i].is_empty() && !added {
            added = true;
            result.push(object!{user: uid, score: score, star_level: star_level}).unwrap();
        }
        if scores[i].is_empty() {
            break;
        }
        if scores[i]["score"].as_i64().unwrap() < score && !added {
            added = true;
            result.push(object!{user: uid, score: score, star_level: star_level}).unwrap();
            current += 1;
            if current >= 10000 {
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
        if DATABASE.lock_and_select("SELECT score_data FROM scores WHERE event_id=?1", params!(event_id)).is_ok() {
            DATABASE.lock_and_exec("UPDATE scores SET score_data=?1 WHERE event_id=?2", params!(json::stringify(result), event_id));
        } else {
            DATABASE.lock_and_exec("INSERT INTO scores (score_data, event_id) VALUES (?1, ?2)", params!(json::stringify(result), event_id));
        }
    }
}

pub fn get_raw_info(event: u32) -> JsonValue {
    let info = DATABASE.lock_and_select("SELECT score_data FROM scores WHERE event_id=?1", params!(event)).unwrap_or(String::from("[]"));
    json::parse(&info).unwrap()
}

fn get_json() -> JsonValue {
    let events = DATABASE.lock_and_select_all("SELECT event_id FROM scores", params!()).unwrap();
    let mut rv = object!{};
    for event in events.members() {
        rv[event.to_string()] = array![];
        let scores = json::parse(&DATABASE.lock_and_select("SELECT score_data FROM scores WHERE event_id=?1", params!(event.as_i64().unwrap())).unwrap()).unwrap();

        let mut i = 1;
        for score in scores.members() {
            let user = global::get_user(score["user"].as_i64().unwrap(), &object![], false);
            rv[event.to_string()].push(object!{
                "rank": i,
                "user_detail": user,
                "score": score["score"].as_i64().unwrap(),
                "star_level": score["star_level"].as_i64().unwrap()
            }).unwrap();
            i += 1;
        }
    }
    object!{
        "cache": rv,
        "last_updated": global::timestamp()
    }
}

pub async fn get_scores_json() -> JsonValue {
    let mut result = crate::lock_onto_mutex!(CACHED_DATA);
    if result.is_none() {
        result.replace(get_json());
    }
    let cache = result.as_ref().unwrap();
    let rv = cache["cache"].clone();
    if cache["last_updated"].as_u64().unwrap() + (60 * 60) < global::timestamp() {
        let mut result = crate::lock_onto_mutex!(CACHED_DATA);
        let new = get_json();
        result.replace(new.clone());
    }
    return rv;
}

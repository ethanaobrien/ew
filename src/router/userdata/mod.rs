pub mod user;

use rusqlite::params;
use lazy_static::lazy_static;
use jzon::{JsonValue, array, object};
use rand::RngExt;

use crate::router::global;
use crate::router::items;
use crate::database::custom_song;
use crate::sql::SQLite;
use crate::include_file;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("userdata.db", setup_tables);
    static ref NEW_USER: JsonValue = {
        let raw = crate::runtime::read_masterdata_file("userdata/new_user.json")
            .and_then(|b| String::from_utf8(b).ok())
            .unwrap_or_else(|| include_file!("src/router/userdata/new_user.json"));
        jzon::parse(&raw).unwrap()
    };
}

fn get_userdata_database() -> &'static SQLite {
    &DATABASE
}

fn setup_tables(conn: &rusqlite::Connection) {
    user::migration::setup_sql(conn).unwrap();
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS tokens (
    user_id           BIGINT NOT NULL PRIMARY KEY,
    token             TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS userdata (
    user_id                  BIGINT NOT NULL PRIMARY KEY,
    userdata                 TEXT NOT NULL,
    friend_request_disabled  INT NOT NULL
);
CREATE TABLE IF NOT EXISTS userhome (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    userhome         TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS missions (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    missions         TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS loginbonus (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    loginbonus       TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS sifcards (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    sifcards         TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS friends (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    friends          TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS chats (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    chats            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS event (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    event            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS eventloginbonus (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    eventloginbonus  TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS server_data (
    user_id          BIGINT NOT NULL PRIMARY KEY,
    server_data      TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS webui (
    user_id      BIGINT NOT NULL PRIMARY KEY,
    token        TEXT NOT NULL,
    last_login   BIGINT NOT NULL
);
    ").unwrap();
}

fn acc_exists(uid: i64) -> bool {
    DATABASE.lock_and_select("SELECT user_id FROM userdata WHERE user_id=?1", params!(uid)).is_ok()
}
fn get_key(auth_key: &str) -> i64 {
    let uid = get_uid(auth_key);
    let key = if uid == 0 {
        generate_uid()
    } else {
        uid
    };
    
    if !acc_exists(key) {
        create_acc(key, auth_key);
    }
    
    key
}

fn generate_uid() -> i64 {
    let mut rng = rand::rng();
    let random_number = rng.random_range(100_000_000_000_000..=999_999_999_999_999);
    //the chances of this...?
    if acc_exists(random_number) {
        return generate_uid();
    }
    
    random_number
}

fn add_user_to_database(uid: i64, user: JsonValue, user_home: JsonValue, user_missions: JsonValue, sif_cards: JsonValue) {
    let home = jzon::stringify(user_home.clone());
    let missions = jzon::stringify(user_missions.clone());
    let cards = jzon::stringify(sif_cards.clone());
    
    DATABASE.lock_and_exec("INSERT INTO userdata (user_id, userdata, friend_request_disabled) VALUES (?1, ?2, ?3)", params!(
        uid,
        jzon::stringify(user.clone()),
        user["user"]["friend_request_disabled"].as_i32().unwrap()
    ));
    DATABASE.lock_and_exec("INSERT INTO userhome (user_id, userhome) VALUES (?1, ?2)", params!(
        uid,
        if user_home.is_empty() {include_file!("src/router/userdata/new_user_home.json")} else {home}
    ));
    DATABASE.lock_and_exec("INSERT INTO missions (user_id, missions) VALUES (?1, ?2)", params!(
        uid,
        if user_missions.is_empty() {include_file!("src/router/userdata/missions.json")} else {missions}
    ));
    DATABASE.lock_and_exec("INSERT INTO loginbonus (user_id, loginbonus) VALUES (?1, ?2)", params!(
        uid,
        format!(r#"{{"last_rewarded": 0, "bonus_list": [], "start_time": {}}}"#, global::timestamp())
    ));
    DATABASE.lock_and_exec("INSERT INTO sifcards (user_id, sifcards) VALUES (?1, ?2)", params!(
        uid,
        if sif_cards.is_empty() {"[]"} else {&cards}
    ));
    DATABASE.lock_and_exec("INSERT INTO friends (user_id, friends) VALUES (?1, ?2)", params!(
        uid,
        r#"{"friend_user_id_list":[],"request_user_id_list":[],"pending_user_id_list":[]}"#
    ));
    DATABASE.lock_and_exec("INSERT INTO event (user_id, event) VALUES (?1, ?2)", params!(
        uid,
        "{}"
    ));
    DATABASE.lock_and_exec("INSERT INTO eventloginbonus (user_id, eventloginbonus) VALUES (?1, ?2)", params!(
        uid,
        format!(r#"{{"last_rewarded": 0, "bonus_list": [], "start_time": {}}}"#, global::timestamp())
    ));
    DATABASE.lock_and_exec("INSERT INTO server_data (user_id, server_data) VALUES (?1, ?2)", params!(
        uid,
        format!(r#"{{"server_time_set":{},"server_time":1709272800}}"#, global::timestamp())
    ));
    DATABASE.lock_and_exec("INSERT INTO chats (user_id, chats) VALUES (?1, ?2)", params!(
        uid,
        "[]"
    ));
}

fn create_acc(uid: i64, login: &str) {
    let mut new_user = NEW_USER.clone();
    new_user["user"]["id"] = uid.into();
    new_user["stamina"]["last_updated_time"] = global::timestamp().into();
    
    add_user_to_database(uid, new_user, JsonValue::Null, JsonValue::Null, JsonValue::Null);
    
    DATABASE.lock_and_exec("DELETE FROM tokens WHERE token=?1", params!(login));
    DATABASE.lock_and_exec("INSERT INTO tokens (user_id, token) VALUES (?1, ?2)", params!(uid, login));
}

fn get_uid(token: &str) -> i64 {
    let data = DATABASE.lock_and_select("SELECT user_id FROM tokens WHERE token = ?1;", params!(token));
    if data.is_err() {
        return 0;
    }
    let data = data.unwrap();
    data.parse::<i64>().unwrap_or(0)
}

// Needed by gree
pub fn get_login_token(uid: i64) -> String {
    let data = DATABASE.lock_and_select("SELECT token FROM tokens WHERE user_id=?1", params!(uid));
    if data.is_err() {
        return String::new();
    }
    data.unwrap()
}

fn cleanup_account(user: &mut JsonValue) {
    user["gem"]["total"] = (user["gem"]["charge"].as_i64().unwrap() + user["gem"]["free"].as_i64().unwrap()).into();
    if user["master_music_ids"].len() != NEW_USER["master_music_ids"].len() {
        user["master_music_ids"] = NEW_USER["master_music_ids"].clone();
    }
    
    let mut to_remove = array![];
    let items = user["item_list"].clone();
    for (i, data) in user["item_list"].members_mut().enumerate() {
        if to_remove.contains(i) {
            continue;
        }
        if data["master_item_id"].as_i64().unwrap() != data["id"].as_i64().unwrap() {
            data["id"] = data["master_item_id"].clone();
        }
        for (j, data2) in items.members().enumerate() {
            if i == j {
                continue;
            }
            if data["master_item_id"].as_i64().unwrap() == data2["master_item_id"].as_i64().unwrap() {
                to_remove.push(j).unwrap();
                data["amount"] = (data["amount"].as_i64().unwrap() + data2["amount"].as_i64().unwrap()).into();
            }
        }
    }
    for (i, data) in to_remove.members().enumerate() {
        user["item_list"].array_remove(data.as_usize().unwrap() - i);
    }
}

fn get_data(auth_key: &str, row: &str) -> JsonValue {
    let key = get_key(auth_key);
    
    let result = DATABASE.lock_and_select(&format!("SELECT {} FROM {} WHERE user_id=?1", row, row), params!(key));
    
    jzon::parse(&result.unwrap()).unwrap()
}

// Deleted custom songs leave stale score/clear records behind. They're wiped
// lazily when the userdata is pulled: collect the user's own music-id-keyed
// rows in the custom range (official ids are never candidates) and drop the
// ones whose id no longer exists in the catalog. Custom ids are never reused,
// so the wipe is final. A song that still exists but isn't visible to this
// user is NOT wiped - existence is what's checked, not visibility
fn remove_deleted_custom_songs(user: &mut JsonValue) -> bool {
    // Feature off: never touch custom_songs.db, leave userdata untouched
    if crate::router::custom_song::disabled() {
        return false;
    }
    let mut candidates = array![];
    for key in ["live_list", "live_mission_list"] {
        for data in user[key].members() {
            let id = data["master_live_id"].as_i64().unwrap_or(0);
            if id >= custom_song::FIRST_MUSIC_ID && !candidates.contains(id) {
                candidates.push(id).unwrap();
            }
        }
    }
    if candidates.is_empty() {
        return false;
    }
    let dead = custom_song::dead_music_ids(&candidates);
    if dead.is_empty() {
        return false;
    }
    for key in ["live_list", "live_mission_list"] {
        let mut i = 0;
        while i < user[key].len() {
            if dead.contains(user[key][i]["master_live_id"].as_i64().unwrap_or(0)) {
                user[key].array_remove(i);
            } else {
                i += 1;
            }
        }
    }
    true
}

pub fn get_acc(auth_key: &str) -> JsonValue {
    let mut user = get_data(auth_key, "userdata");
    cleanup_account(&mut user);
    if remove_deleted_custom_songs(&mut user) {
        save_data(auth_key, "userdata", user.clone());
    }

    items::lp_modification(&mut user, 0, false);
    user
}

pub fn get_acc_home(auth_key: &str) -> JsonValue {
    let mut user = get_data(auth_key, "userhome");
    user["home"]["pending_friend_count"] = get_acc_friends(auth_key)["pending_user_id_list"].len().into();
    
    user
}
pub fn get_acc_missions(auth_key: &str) -> JsonValue {
    get_data(auth_key, "missions")
}
pub fn get_acc_loginbonus(auth_key: &str) -> JsonValue {
    get_data(auth_key, "loginbonus")
}
pub fn get_acc_sif(auth_key: &str) -> JsonValue {
    get_data(auth_key, "sifcards")
}
pub fn get_acc_friends(auth_key: &str) -> JsonValue {
    get_data(auth_key, "friends")
}
pub fn get_server_data(auth_key: &str) -> JsonValue {
    get_data(auth_key, "server_data")
}
pub fn get_acc_chats(auth_key: &str) -> JsonValue {
    get_data(auth_key, "chats")
}
pub fn get_acc_event(auth_key: &str) -> JsonValue {
    let event = get_data(auth_key, "event");
    if event.is_empty() {
        return object!{};
    }
    event
}
pub fn get_acc_eventlogin(auth_key: &str) -> JsonValue {
    get_data(auth_key, "eventloginbonus")
}

pub fn save_data(auth_key: &str, row: &str, data: JsonValue) {
    let key = get_key(auth_key);
    
    DATABASE.lock_and_exec(&format!("UPDATE {} SET {}=?1 WHERE user_id=?2", row, row), params!(jzon::stringify(data), key));
}

pub fn save_acc(auth_key: &str, data: JsonValue) {
    DATABASE.lock_and_exec("UPDATE userdata SET friend_request_disabled=?1 WHERE user_id=?2", params!(data["user"]["friend_request_disabled"].as_i32().unwrap(), get_key(auth_key)));
    save_data(auth_key, "userdata", data);
}
pub fn save_acc_home(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "userhome", data);
}
pub fn save_acc_missions(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "missions", data);
}
pub fn save_acc_loginbonus(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "loginbonus", data);
}
pub fn save_acc_friends(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "friends", data);
}
pub fn save_acc_event(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "event", data);
}
pub fn save_acc_eventlogin(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "eventloginbonus", data);
}
pub fn save_server_data(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "server_data", data);
}
pub fn save_acc_chats(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "chats", data);
}
pub fn save_acc_sif(auth_key: &str, data: JsonValue) {
    save_data(auth_key, "sifcards", data);
}

pub fn get_name_and_rank(uid: i64) -> JsonValue {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return object!{
            user_name: "",
            user_rank: 1
        }
    }
    let uid = get_uid(&login_token);
    if uid == 0 || !acc_exists(uid) {
        return object!{
            user_name: "",
            user_rank: 1
        }
    }
    let result = DATABASE.lock_and_select("SELECT userdata FROM userdata WHERE user_id=?1", params!(uid));
    let data = jzon::parse(&result.unwrap()).unwrap();
    
    object!{
        user_name: data["user"]["name"].clone(),
        user_rank: items::get_user_rank_data(data["user"]["exp"].as_i64().unwrap())["rank"].clone() //todo
    }
}

pub fn get_acc_from_uid(uid: i64) -> JsonValue {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return object!{
            error: true
        }
    }
    let uid = get_uid(&login_token);
    if uid == 0 || !acc_exists(uid) {
        return object!{"error": true}
    }
    let result = DATABASE.lock_and_select("SELECT userdata FROM userdata WHERE user_id=?1", params!(uid));
    jzon::parse(&result.unwrap()).unwrap()
}

pub fn friend_request(uid: i64, requestor: i64) {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return;
    }
    let uid = get_uid(&login_token);
    let friends = DATABASE.lock_and_select("SELECT friends FROM friends WHERE user_id=?1", params!(uid));
    let mut friends = jzon::parse(&friends.unwrap()).unwrap();
    if !friends["pending_user_id_list"].contains(requestor) && friends["pending_user_id_list"].len() < crate::router::friend::FRIEND_LIMIT {
        friends["pending_user_id_list"].push(requestor).unwrap();
        DATABASE.lock_and_exec("UPDATE friends SET friends=?1 WHERE user_id=?2", params!(jzon::stringify(friends), uid));
    }
}

pub fn friend_request_approve(uid: i64, requestor: i64, accepted: bool, key: &str) {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return;
    }
    let uid = get_uid(&login_token);
    let friends = DATABASE.lock_and_select("SELECT friends FROM friends WHERE user_id=?1", params!(uid));
    let mut friends = jzon::parse(&friends.unwrap()).unwrap();
    let index = friends[key].members().position(|r| *r.to_string() == requestor.to_string());
    if index.is_some() {
        friends[key].array_remove(index.unwrap());
    }
    let index = friends["request_user_id_list"].members().position(|r| *r.to_string() == requestor.to_string());
    if index.is_some() {
        friends["request_user_id_list"].array_remove(index.unwrap());
    }
    if accepted && !friends["friend_user_id_list"].contains(requestor) && friends["friend_user_id_list"].len() < crate::router::friend::FRIEND_LIMIT {
        friends["friend_user_id_list"].push(requestor).unwrap();
    }
    DATABASE.lock_and_exec("UPDATE friends SET friends=?1 WHERE user_id=?2", params!(jzon::stringify(friends), uid));
}

pub fn friend_request_disabled(uid: i64) -> bool {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return true;
    }
    let uid = get_uid(&login_token);
    let user = DATABASE.lock_and_select("SELECT userdata FROM userdata WHERE user_id=?1", params!(uid));
    let user = jzon::parse(&user.unwrap()).unwrap();
    user["user"]["friend_request_disabled"] == 1
}

pub fn friend_remove(uid: i64, requestor: i64) {
    let login_token = get_login_token(uid);
    if login_token == String::new() {
        return;
    }
    let uid = get_uid(&login_token);
    let friends = DATABASE.lock_and_select("SELECT friends FROM friends WHERE user_id=?1", params!(uid));
    let mut friends = jzon::parse(&friends.unwrap()).unwrap();
    let index = friends["friend_user_id_list"].members().position(|r| *r.to_string() == requestor.to_string());
    if index.is_some() {
        friends["friend_user_id_list"].array_remove(index.unwrap());
    }
    DATABASE.lock_and_exec("UPDATE friends SET friends=?1 WHERE user_id=?2", params!(jzon::stringify(friends), uid));
}

pub fn get_random_uids(count: i32) -> JsonValue {
    if count <= 0 {
        return array![];
    }
    DATABASE.lock_and_select_all(&format!("SELECT user_id FROM userdata WHERE friend_request_disabled=?1 ORDER BY RANDOM() LIMIT {}", count), params!(0)).unwrap()
}

fn create_webui_token() -> String {
    let token = global::create_token();
    if DATABASE.lock_and_select("SELECT user_id FROM webui WHERE token=?1", params!(token)).is_ok() {
        return create_webui_token();
    }
    token
}

pub fn webui_login(uid: i64, password: &str) -> Result<String, String> {
    let pass = DATABASE.lock_and_select("SELECT password FROM migration WHERE user_id=?1", params!(uid)).unwrap_or_default();
    if !user::migration::verify_password(password, &pass) {
        if acc_exists(uid) && pass.is_empty() {
            return Err(String::from("Migration token not set. Set token in game settings."));
        }
        return Err(String::from("User/password don't match"));
    }
    
    let new_token = create_webui_token();
    
    DATABASE.lock_and_exec("DELETE FROM webui WHERE user_id=?1", params!(uid));
    // This could overflow given enough time... though rusqlite doesnt currently support ToSql on a u64...... maybe someday
    DATABASE.lock_and_exec("INSERT INTO webui (user_id, token, last_login) VALUES (?1, ?2, ?3)", params!(uid, new_token, global::timestamp() as i64));
    Ok(new_token)
}

pub fn webui_import_user(user: JsonValue) -> Result<JsonValue, String> {
    let uid = user["userdata"]["user"]["id"].as_i64().unwrap();
    if acc_exists(uid) {
        return Err(String::from("User already exists"));
    }
    
    add_user_to_database(uid, user["userdata"].clone(), user["home"].clone(), user["missions"].clone(), user["sif_cards"].clone());
    
    let token = global::create_token();
    
    DATABASE.lock_and_exec("INSERT INTO tokens (user_id, token) VALUES (?1, ?2)", params!(uid, token));
    
    let token = user::migration::save_acc_transfer(uid, &user["password"].to_string());
    
    Ok(object!{
        uid: uid,
        migration_token: token
    })
}

pub fn webui_login_token(token: &str) -> Option<String> {
    let uid = DATABASE.lock_and_select("SELECT user_id FROM webui WHERE token=?1", params!(token)).unwrap_or_default();
    if uid == String::new() || token.is_empty() {
        return None;
    }
    let uid = uid.parse::<i64>().unwrap_or(0);
    if uid == 0 {
        return None;
    }
    let last_login = DATABASE.lock_and_select("SELECT last_login FROM webui WHERE user_id=?1", params!(uid)).unwrap_or_default().parse::<i64>().unwrap_or(0);
    let limit = 24 * 60 * 60; //1 day
    //Expired token
    if (global::timestamp() as i64) > last_login + limit {
        DATABASE.lock_and_exec("DELETE FROM webui WHERE user_id=?1", params!(uid));
        return None;
    }
    let login_token = DATABASE.lock_and_select("SELECT token FROM tokens WHERE user_id=?1", params!(uid)).unwrap_or_default();
    if login_token == String::new() {
        return None;
    }
    Some(login_token)
}

pub fn webui_get_user(token: &str) -> Option<JsonValue> {
    let login_token = webui_login_token(token)?;
    
    Some(object!{
        userdata: get_acc(&login_token),
        loginbonus: get_acc_loginbonus(&login_token),
        time: get_server_data(&login_token)["server_time"].clone()
    })
}

pub fn webui_start_loginbonus(bonus_id: i64, token: &str) -> JsonValue {
    let login_token = webui_login_token(token);
    if login_token.is_none() {
        return object!{
            result: "ERR",
            message: "Failed to validate token"
        };
    }
    let login_token = login_token.unwrap();
    let mut bonuses = get_acc_loginbonus(&login_token);
    if !global::start_login_bonus(bonus_id, &mut bonuses) {
        return object!{
            result: "ERR",
            message: "Login bonus ID is either already going or does not exist"
        };
    }
    save_acc_loginbonus(&login_token, bonuses);
    
    object!{
        result: "OK",
        id: bonus_id
    }
}

pub fn set_server_time(time: i64, token: &str) -> JsonValue {
    if time as u64 > global::timestamp() {
        return object!{
            result: "ERR",
            message: "Timestamp is in the future"
        };
    }
    let login_token = webui_login_token(token);
    if login_token.is_none() {
        return object!{
            result: "ERR",
            message: "Failed to validate token"
        };
    }
    let login_token = login_token.unwrap();
    let mut server_data = get_server_data(&login_token);
    server_data["server_time_set"] = global::timestamp().into();
    server_data["server_time"] = time.into();
    save_server_data(&login_token, server_data);
    
    object!{
        result: "OK"
    }
}

pub fn webui_logout(token: &str) {
    DATABASE.lock_and_exec("DELETE FROM webui WHERE token=?1", params!(token));
}

pub fn export_user(token: &str) -> Option<JsonValue> {
    let login_token = webui_login_token(token)?;

    Some(object!{
         userdata: jzon::stringify(get_acc(&login_token)),
         userhome: jzon::stringify(get_acc_home(&login_token)),
         missions: jzon::stringify(get_acc_missions(&login_token)),
         sifcards: jzon::stringify(get_acc_sif(&login_token))
    })
}

pub fn purge_accounts() -> usize {
    // If the user has no cards, its safe to assume its a dead account (imo). In the (rare) event this function is ran after a user started and before the account has characters, the server should create them a new account, and let them start the tutorial over.
    let dead_uids = DATABASE.lock_and_select_all("
    SELECT user_id
    FROM userdata
    WHERE (userdata LIKE ?1 AND userdata LIKE ?2 AND friend_request_disabled=1)
    OR (userdata LIKE ?3 AND friend_request_disabled=1)",
    params!(
        "%\"card_list\":[]%",
        "%Tutorial in progress%",
        "%tutorial_step\":60%" //For some reason, a majority of dead accounts in the tutorial are here....
    )).unwrap();
    for uid in dead_uids.members() {
        let user_id = uid.as_i64().unwrap();
        println!("Removing dead UID: {}", user_id);
        crate::database::gree::delete_uuid(user_id);
        DATABASE.lock_and_exec("DELETE FROM userdata WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM userhome WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM missions WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM loginbonus WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM sifcards WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM friends WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM chats WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM event WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM eventloginbonus WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM server_data WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM webui WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM tokens WHERE user_id=?1", params!(user_id));
        DATABASE.lock_and_exec("DELETE FROM migration WHERE user_id=?1", params!(user_id));
    }
    DATABASE.lock_and_exec("VACUUM", params!());
    crate::database::gree::setup();
    dead_uids.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    // User plays a custom song -> the song is deleted -> the next userdata
    // pull serves data without the dead scores and the stored rows are gone
    #[test]
    fn deleted_custom_song_records_are_wiped_on_pull() {
        let _lock = crate::runtime::lock_test_data_path();

        let token = "userdata-test-token";
        let mut user = get_acc(token);

        let deleted_id = custom_song::next_music_id();
        custom_song::insert_song(deleted_id, 1, &object!{music_id: deleted_id}, "public", &array![], false);
        // Exists but isn't visible to this user - must survive the wipe
        let private_id = custom_song::next_music_id();
        custom_song::insert_song(private_id, 1, &object!{music_id: private_id}, "private", &array![], false);

        for id in [1001, deleted_id, private_id] {
            user["live_list"].push(object!{
                master_live_id: id,
                level: 4,
                clear_count: 1,
                high_score: 123456,
                max_combo: 100
            }).unwrap();
            user["live_mission_list"].push(object!{
                master_live_id: id,
                clear_master_live_mission_ids: [1, 24]
            }).unwrap();
        }
        save_acc(token, user);

        // Both custom songs still exist: nothing gets wiped
        let user = get_acc(token);
        assert_eq!(user["live_list"].len(), 3);
        assert_eq!(user["live_mission_list"].len(), 3);

        custom_song::delete_song(deleted_id);

        // The next pull drops the dead id's records and only those
        let user = get_acc(token);
        assert!(!user["live_list"].members().any(|data| data["master_live_id"] == deleted_id));
        assert!(!user["live_mission_list"].members().any(|data| data["master_live_id"] == deleted_id));
        // Official records are untouchable, invisible-but-alive songs survive
        assert!(user["live_list"].members().any(|data| data["master_live_id"] == 1001));
        assert!(user["live_list"].members().any(|data| data["master_live_id"] == private_id));
        assert_eq!(user["live_list"].len(), 2);
        assert_eq!(user["live_mission_list"].len(), 2);

        // The wipe persisted to the database, not just the served copy
        let stored = get_data(token, "userdata");
        assert!(!stored["live_list"].members().any(|data| data["master_live_id"] == deleted_id));
        assert!(!stored["live_mission_list"].members().any(|data| data["master_live_id"] == deleted_id));
    }
}

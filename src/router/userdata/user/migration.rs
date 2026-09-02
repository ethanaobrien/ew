use argon2::{
    password_hash::{rand_core::OsRng, PasswordHash, PasswordHasher, PasswordVerifier, SaltString},
    Argon2
};
use rusqlite::params;
use jzon::{JsonValue, object};
use crate::router::userdata;
use rand::RngExt;
use sha2::{Digest, Sha256};
use base64::{Engine as _, engine::general_purpose};

fn generate_token() -> String {
    let charset = "ABCDEFGHIJKLMNOPQRSTUVWXYZ0123456789";
    let mut rng = rand::rng();
    loop {
        let random_string: String = (0..16)
            .map(|_| {
                let idx = rng.random_range(0..charset.len());
                charset.chars().nth(idx).unwrap()
            })
            .collect();
        if !random_string.chars().all(|c| c.is_ascii_digit()) {
            return random_string;
        }
    }
}

// A linked NESiCA card id works as the transfer code; the password is still required
pub fn get_acc_transfer(token: &str, password: &str) -> JsonValue {
    let database = userdata::get_userdata_database();
    let uid: i64 = if let Ok(hash) = database.lock_and_select("SELECT password FROM migration WHERE token=?1", params!(token)) {
        if !verify_password(password, &hash) {
            return object!{success: false};
        }
        database.lock_and_select_type("SELECT user_id FROM migration WHERE token=?1", params!(token)).unwrap()
    } else {
        let Some(uid) = valid_card_id(token).and_then(|card| card_user(&card)) else {
            return object!{success: false};
        };
        let Ok(hash) = database.lock_and_select("SELECT password FROM migration WHERE user_id=?1", params!(uid)) else {
            return object!{success: false};
        };
        if !verify_password(password, &hash) {
            return object!{success: false};
        }
        uid
    };
    let login_token = userdata::get_login_token(uid);
    if login_token == String::new() {
        return object!{success: false};
    }
    object!{success: true, login_token: login_token, user_id: uid}
}

// Used by gree
pub fn transfer_code_exists(token: &str) -> bool {
    let database = userdata::get_userdata_database();
    database.lock_and_select("SELECT password FROM migration WHERE token=?1", params!(token)).is_ok()
        || valid_card_id(token).and_then(|card| card_user(&card)).is_some()
}

pub fn valid_card_id(card_id: &str) -> Option<String> {
    let card_id = card_id.trim().to_string();
    if card_id.is_empty() || card_id.len() > 32 || !card_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(card_id)
}

pub fn card_user(card_id: &str) -> Option<i64> {
    userdata::get_userdata_database().lock_and_select_type("SELECT user_id FROM cards WHERE card_id=?1", params!(card_id)).ok()
}

pub fn cards_of_account(user_id: i64) -> Vec<String> {
    let Ok(conn) = rusqlite::Connection::open(userdata::get_userdata_database().get_path()) else { return Vec::new(); };
    let Ok(mut stmt) = conn.prepare("SELECT card_id FROM cards WHERE user_id=?1 ORDER BY created ASC") else { return Vec::new(); };
    let Ok(rows) = stmt.query_map(params!(user_id), |row| row.get::<usize, String>(0)) else { return Vec::new(); };
    rows.flatten().collect()
}

pub fn account_has_card(user_id: i64) -> bool {
    userdata::get_userdata_database().lock_and_select("SELECT card_id FROM cards WHERE user_id=?1", params!(user_id)).is_ok()
}

pub fn set_card(card_id: &str, user_id: i64) {
    userdata::get_userdata_database().lock_and_exec(
        "INSERT INTO cards (card_id, user_id, created) VALUES (?1, ?2, ?3) ON CONFLICT(card_id) DO UPDATE SET user_id=?2",
        params!(card_id, user_id, crate::router::global::timestamp() as i64)
    );
}

pub fn import_card(card_id: &str, user_id: i64, created: i64) {
    userdata::get_userdata_database().lock_and_exec(
        "INSERT OR IGNORE INTO cards (card_id, user_id, created) VALUES (?1, ?2, ?3)",
        params!(card_id, user_id, created)
    );
}

pub fn remove_card(card_id: &str) {
    userdata::get_userdata_database().lock_and_exec("DELETE FROM cards WHERE card_id=?1", params!(card_id));
}

pub fn remove_card_of(card_id: &str, user_id: i64) -> bool {
    match card_user(card_id) {
        Some(owner) if owner == user_id => {
            remove_card(card_id);
            true
        }
        _ => false
    }
}

// The one rule for linking a card, whoever proved the account (game session, webui session, cabinet transfer pair)
pub fn link_card(card_id: &str, user_id: i64) -> Result<(String, bool), String> {
    let Some(card) = valid_card_id(card_id) else {
        return Err(String::from("That is not a usable card id"));
    };
    if crate::router::arcade::is_cabinet_account(user_id) {
        return Err(String::from("That account belongs to an arcade cabinet"));
    }
    let previous = card_user(&card);
    set_card(&card, user_id);
    crate::router::arcade::card_relinked(&card);
    Ok((card, previous.is_some_and(|p| p != user_id)))
}

pub fn save_acc_transfer(uid: i64, password: &str) -> String {
    let database = userdata::get_userdata_database();
    let token = if let Ok(value) = database.lock_and_select("SELECT token FROM migration WHERE user_id=?1", params!(uid)) {
        value
    } else {
        generate_token()
    };
    database.lock_and_exec("DELETE FROM migration WHERE user_id=?1", params!(uid));
    database.lock_and_exec("INSERT INTO migration (user_id, token, password) VALUES (?1, ?2, ?3)", params!(uid, &token, hash_password(password)));
    token
}

pub fn get_acc_token(uid: i64) -> String {
    let database = userdata::get_userdata_database();
    if let Ok(value) = database.lock_and_select("SELECT token FROM migration WHERE user_id=?1", params!(uid)) {
        value
    } else {
        save_acc_transfer(uid, "")
    }
}

fn hash_password(password: &str) -> String {
    if password.is_empty() { return String::new(); }
    let salt = SaltString::generate(&mut OsRng);
    Argon2::default()
        .hash_password(password.as_bytes(), &salt)
        .unwrap()
        .to_string()
}

pub fn verify_password(password: &str, hash: &str) -> bool {
    if password.is_empty() || hash.is_empty() {
        return false;
    }
    if !hash.starts_with("$argon2") {
        return legacy_verify_password(password, hash);
    }
    let parsed = match PasswordHash::new(hash) {
        Ok(h) => h,
        Err(_) => return false,
    };
    Argon2::default().verify_password(password.as_bytes(), &parsed).is_ok()
}

pub fn setup_sql(conn: &rusqlite::Connection) -> Result<(), rusqlite::Error> {
    conn.execute("
    CREATE TABLE IF NOT EXISTS migration (
        user_id           BIGINT NOT NULL,
        token             TEXT NOT NULL,
        password          TEXT NOT NULL,
        PRIMARY KEY (user_id, token)
    );
    ", [])?;
    conn.execute("
    CREATE TABLE IF NOT EXISTS cards (
        card_id           TEXT NOT NULL PRIMARY KEY,
        user_id           BIGINT NOT NULL,
        created           BIGINT NOT NULL
    );
    ", [])?;
    let is_updated = conn.prepare("SELECT user_id FROM migration LIMIT 1;").is_ok();
    if is_updated { return Ok(()); }
    println!("Upgrading migration table");
    conn.execute("DROP TABLE IF EXISTS migration_new;", [])?;
    conn.execute("
    CREATE TABLE migration_new (
        user_id           BIGINT NOT NULL,
        token             TEXT NOT NULL,
        password          TEXT NOT NULL,
        PRIMARY KEY (user_id, token)
    );
    ", [])?;
    
    let mut stmt = conn.prepare("SELECT token, password FROM migration")?;

    let rows = stmt.query_map([], |row| {
        Ok((
            row.get::<_, String>(0)?,
            row.get::<_, String>(1)?,
        ))
    })?;

    let mut insert_new_row = conn.prepare("INSERT INTO migration_new (user_id, token, password) VALUES (?1, ?2, ?3)")?;
    for row in rows {
        let (token, password) = row?;

        let user_id = code_to_uid(&token);

        insert_new_row.execute(params![user_id, token, password])?;
    }
    conn.execute("DROP TABLE migration;", params!())?;
    conn.execute("ALTER TABLE migration_new RENAME TO migration;", params!())?;
    Ok(())
}

// For migrating from a legacy database
fn code_to_uid(code: &str) -> String {
    code
        .replace('7', "")
        .replace('A', "1")
        .replace('G', "2")
        .replace('W', "3")
        .replace('Q', "4")
        .replace('Y', "5")
        .replace('6', "6")
        .replace('I', "7")
        .replace('P', "8")
        .replace('U', "9")
        .replace('M', "0")
}

fn legacy_verify_password(password: &str, salted_hash: &str) -> bool {
    let bytes = match general_purpose::STANDARD.decode(salted_hash) {
        Ok(b) if b.len() >= 17 => b,
        _ => return password == salted_hash,
    };
    let (salt, hashed_password) = bytes.split_at(16);
    let hashed_password = &hashed_password[0..32];
    let mut hasher = Sha256::new();
    hasher.update(password.as_bytes());
    hasher.update(salt);
    let input_hash = hasher.finalize();
    input_hash.as_slice() == hashed_password
}

// whenever an ai touches this codebase it decides to write 200000 lines of tests

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn an_account_lists_and_unlinks_only_its_own_cards() {
        let _lock = crate::runtime::lock_test_data_path();

        let (mine, _) = userdata::starter::create("Mine").unwrap();
        let (theirs, _) = userdata::starter::create("Theirs").unwrap();
        assert!(cards_of_account(mine).is_empty());

        set_card("7020392000000011", mine);
        set_card("7020392000000012", mine);
        set_card("7020392000000013", theirs);
        assert_eq!(cards_of_account(mine), vec!["7020392000000011".to_string(), "7020392000000012".to_string()]);
        assert_eq!(cards_of_account(theirs), vec!["7020392000000013".to_string()]);
        assert!(account_has_card(mine));

        assert!(!remove_card_of("7020392000000013", mine));
        assert_eq!(card_user("7020392000000013"), Some(theirs));
        assert!(!remove_card_of("7020392000000014", mine));

        assert!(remove_card_of("7020392000000011", mine));
        assert!(card_user("7020392000000011").is_none());
        assert_eq!(cards_of_account(mine), vec!["7020392000000012".to_string()]);

        remove_card("7020392000000012");
        remove_card("7020392000000013");
        assert!(!account_has_card(mine));
        userdata::delete_account(mine);
        userdata::delete_account(theirs);
    }

    #[test]
    fn card_ids_are_validated_not_sanitised() {
        assert_eq!(valid_card_id("0123456789012345").as_deref(), Some("0123456789012345"));
        assert_eq!(valid_card_id(" 0123456789012345 ").as_deref(), Some("0123456789012345"));
        assert!(valid_card_id("").is_none());
        assert!(valid_card_id("0123-4567").is_none());
        assert!(valid_card_id("'; DROP TABLE cards--").is_none());
        assert!(valid_card_id(&"x".repeat(33)).is_none());
    }

    #[test]
    fn link_card_repoints_a_linked_card_and_says_so() {
        let _lock = crate::runtime::lock_test_data_path();

        let (first, _) = userdata::starter::create("First").unwrap();
        let (second, _) = userdata::starter::create("Second").unwrap();
        let card = "7020392000000031";

        assert!(link_card("7020-3920", first).is_err());
        assert_eq!(link_card(card, first), Ok((card.to_string(), false)));
        assert_eq!(link_card(card, first), Ok((card.to_string(), false)));
        assert_eq!(link_card(card, second), Ok((card.to_string(), true)));
        assert_eq!(card_user(card), Some(second));
        assert!(!userdata::get_acc_from_uid(first)["error"].as_bool().unwrap_or(false));

        remove_card(card);
        userdata::delete_account(first);
        userdata::delete_account(second);
    }

    #[test]
    fn a_linked_card_is_a_transfer_code_with_the_password_still_required() {
        let _lock = crate::runtime::lock_test_data_path();

        let (player, token) = userdata::starter::create("Card Transfer").unwrap();
        let card = "7020392000000021";

        assert!(!get_acc_transfer(card, "hunter2")["success"].as_bool().unwrap());
        assert!(!transfer_code_exists(card));

        set_card(card, player);
        assert!(!get_acc_transfer(card, "hunter2")["success"].as_bool().unwrap());
        assert!(!get_acc_transfer(card, "")["success"].as_bool().unwrap());
        assert!(transfer_code_exists(card));

        save_acc_transfer(player, "hunter2");
        let by_card = get_acc_transfer(card, "hunter2");
        assert!(by_card["success"].as_bool().unwrap());
        assert_eq!(by_card["user_id"].as_i64(), Some(player));
        assert_eq!(by_card["login_token"].as_str(), Some(token.as_str()));
        assert!(!get_acc_transfer(card, "wrong")["success"].as_bool().unwrap());
        let code = get_acc_token(player);
        assert!(get_acc_transfer(&code, "hunter2")["success"].as_bool().unwrap());
        assert!(!code.chars().all(|c| c.is_ascii_digit()));

        remove_card(card);
        assert!(!get_acc_transfer(card, "hunter2")["success"].as_bool().unwrap());

        userdata::delete_account(player);
    }
}

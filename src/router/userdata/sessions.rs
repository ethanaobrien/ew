use rusqlite::{params, Connection, OptionalExtension};
use jzon::{JsonValue, array, object};
use sha2::{Digest, Sha256};

use crate::router::{global, userdata};

const WEBUI_TTL: i64 = 24 * 60 * 60; // 1 day
const TOUCH_INTERVAL: i64 = 5 * 60;
const UNUSED_TTL: i64 = 24 * 60 * 60;

pub enum Lookup {
    Active(i64, String),
    Revoked,
    Unknown
}

pub fn setup_sql(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS sessions (
    id          INTEGER PRIMARY KEY AUTOINCREMENT,
    user_id     BIGINT NOT NULL,
    token_hash  TEXT NOT NULL UNIQUE,
    kind        TEXT NOT NULL,
    platform    TEXT NOT NULL DEFAULT '',
    created     BIGINT NOT NULL,
    last_seen   BIGINT NOT NULL DEFAULT 0,
    revoked     BIGINT NOT NULL DEFAULT 0
);
CREATE INDEX IF NOT EXISTS sessions_user ON sessions (user_id);
DROP TABLE IF EXISTS webui;
    ")?;
    if conn.prepare("SELECT last_seen FROM tokens LIMIT 1").is_err() {
        conn.execute("ALTER TABLE tokens ADD COLUMN last_seen BIGINT NOT NULL DEFAULT 0", [])?;
    }
    Ok(())
}

fn hash(token: &str) -> String {
    hex::encode(Sha256::digest(token.as_bytes()))
}

fn now() -> i64 {
    global::timestamp() as i64
}

pub fn add(token: &str, user_id: i64, kind: &str, platform: &str) {
    userdata::get_userdata_database().lock_and_exec(
        "INSERT OR IGNORE INTO sessions (user_id, token_hash, kind, platform, created) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!(user_id, hash(token), kind, platform, now())
    );
}

pub fn mint(user_id: i64, kind: &str, platform: &str) -> String {
    userdata::get_userdata_database().lock_and_exec(
        "DELETE FROM sessions WHERE user_id=?1 AND last_seen=0 AND revoked=0 AND created<?2",
        params!(user_id, now() - UNUSED_TTL)
    );
    let token = global::create_token();
    add(&token, user_id, kind, platform);
    token
}

fn find(conn: &Connection, token: &str, platform: Option<&str>) -> rusqlite::Result<Lookup> {
    let row = conn.query_row(
        "SELECT id, user_id, kind, created, last_seen, revoked FROM sessions WHERE token_hash=?1",
        params!(hash(token)),
        |row| Ok((row.get::<_, i64>(0)?, row.get::<_, i64>(1)?, row.get::<_, String>(2)?, row.get::<_, i64>(3)?, row.get::<_, i64>(4)?, row.get::<_, i64>(5)?))
    ).optional()?;
    let Some((id, user_id, kind, created, last_seen, revoked)) = row else {
        return Ok(Lookup::Unknown);
    };
    let now = now();
    if revoked > 0 || (kind == "webui" && created + WEBUI_TTL < now) {
        return Ok(Lookup::Revoked);
    }
    if let Some(platform) = platform && last_seen + TOUCH_INTERVAL < now {
        conn.execute(
            "UPDATE sessions SET last_seen=?1, platform=CASE WHEN ?2='' THEN platform ELSE ?2 END WHERE id=?3",
            params!(now, platform, id)
        )?;
    }
    Ok(Lookup::Active(user_id, kind))
}

pub fn lookup(token: &str) -> Lookup {
    if token.is_empty() {
        return Lookup::Unknown;
    }
    userdata::get_userdata_database().lock_and_transact(|conn| find(conn, token, None)).unwrap_or(Lookup::Unknown)
}

// The one place a client's token becomes the account key every handler uses.
// None means the token was signed out.
pub fn resolve(token: &str, platform: &str) -> Option<String> {
    if token.is_empty() {
        return Some(String::new());
    }
    let resolved = userdata::get_userdata_database().lock_and_transact(|conn| {
        let account = conn.query_row(
            "SELECT last_seen FROM tokens WHERE token=?1",
            params!(token),
            |row| row.get::<_, i64>(0)
        ).optional()?;
        if let Some(last_seen) = account {
            if last_seen + TOUCH_INTERVAL < now() {
                conn.execute("UPDATE tokens SET last_seen=?1 WHERE token=?2", params!(now(), token))?;
            }
            return Ok(Some(token.to_string()));
        }
        match find(conn, token, Some(platform))? {
            Lookup::Active(user_id, kind) if kind != "webui" => conn.query_row(
                "SELECT token FROM tokens WHERE user_id=?1",
                params!(user_id),
                |row| row.get::<_, String>(0)
            ).optional(),
            Lookup::Unknown => Ok(Some(token.to_string())),
            _ => Ok(None)
        }
    });
    resolved.unwrap_or_else(|err| {
        println!("sessions: could not resolve a token: {}", err);
        None
    })
}

pub fn webui_user(token: &str) -> Option<i64> {
    if token.is_empty() {
        return None;
    }
    match userdata::get_userdata_database().lock_and_transact(|conn| find(conn, token, Some(""))) {
        Ok(Lookup::Active(user_id, kind)) if kind == "webui" => Some(user_id),
        _ => None
    }
}

pub fn remove(token: &str) {
    userdata::get_userdata_database().lock_and_exec("DELETE FROM sessions WHERE token_hash=?1", params!(hash(token)));
}

pub fn revoke_token(token: &str) {
    userdata::get_userdata_database().lock_and_exec(
        "UPDATE sessions SET revoked=?1 WHERE token_hash=?2 AND revoked=0",
        params!(now(), hash(token))
    );
}

pub fn revoke(user_id: i64, id: i64) -> bool {
    let conn = Connection::open(userdata::get_userdata_database().get_path()).unwrap();
    conn.execute(
        "UPDATE sessions SET revoked=?1 WHERE id=?2 AND user_id=?3 AND revoked=0",
        params!(now(), id, user_id)
    ).unwrap_or(0) > 0
}

// Also used by the arcade reset, inside its own transaction
pub fn revoke_all_in(conn: &Connection, user_id: i64, keep: &str) -> rusqlite::Result<()> {
    conn.execute(
        "UPDATE sessions SET revoked=?1 WHERE user_id=?2 AND revoked=0 AND token_hash!=?3",
        params!(now(), user_id, hash(keep))
    )?;
    let old = conn.query_row("SELECT token FROM tokens WHERE user_id=?1", params!(user_id), |row| row.get::<_, String>(0)).optional()?;
    if let Some(old) = old {
        conn.execute(
            "INSERT OR IGNORE INTO sessions (user_id, token_hash, kind, created, revoked) VALUES (?1, ?2, 'account', ?3, ?3)",
            params!(user_id, hash(&old), now())
        )?;
    }
    Ok(())
}

pub fn revoke_all(user_id: i64, keep: &str) -> bool {
    let rotated = userdata::get_userdata_database().lock_and_transact(|conn| {
        revoke_all_in(conn, user_id, keep)?;
        conn.execute("UPDATE tokens SET token=?1, last_seen=0 WHERE user_id=?2", params!(global::create_token(), user_id))
    });
    if rotated.is_err() {
        return false;
    }
    crate::database::gree::delete_uuid(user_id);
    true
}

pub fn list(user_id: i64, current: &str) -> JsonValue {
    let mut rv = array![];
    let database = userdata::get_userdata_database();
    let account_seen = database.lock_and_select_type::<i64>("SELECT last_seen FROM tokens WHERE user_id=?1", params!(user_id)).unwrap_or(0);
    if account_seen > 0 {
        rv.push(object!{
            id: 0,
            kind: "account",
            platform: "",
            created: 0,
            last_seen: account_seen,
            current: false
        }).unwrap();
    }
    let Ok(conn) = Connection::open(database.get_path()) else { return rv; };
    let Ok(mut stmt) = conn.prepare(
        "SELECT id, kind, platform, created, last_seen, token_hash FROM sessions
         WHERE user_id=?1 AND revoked=0 AND kind!='account' AND (last_seen>0 OR kind='webui' OR token_hash=?2)
         AND (kind!='webui' OR created>?3) ORDER BY last_seen DESC"
    ) else { return rv; };
    let current = hash(current);
    let Ok(rows) = stmt.query_map(params!(user_id, current, now() - WEBUI_TTL), |row| {
        Ok(object!{
            id: row.get::<_, i64>(0)?,
            kind: row.get::<_, String>(1)?,
            platform: row.get::<_, String>(2)?,
            created: row.get::<_, i64>(3)?,
            last_seen: row.get::<_, i64>(4)?,
            current: row.get::<_, String>(5)? == current
        })
    }) else { return rv; };
    for row in rows.flatten() {
        rv.push(row).unwrap();
    }
    rv
}



// rest of file is tests that ai wrote

#[cfg(test)]
mod tests {
    use super::*;

    fn account_count() -> i64 {
        userdata::get_userdata_database().lock_and_select_type("SELECT COUNT(*) FROM userdata", params!()).unwrap()
    }

    // A token the server has never seen still creates an account, but it is
    // the device's own session from then on and the account key stays internal
    #[test]
    fn a_new_device_gets_its_own_session() {
        let _lock = crate::runtime::lock_test_data_path();
        let device = global::create_token();
        let uid = userdata::get_acc(&device)["user"]["id"].as_i64().unwrap();

        let account_key = userdata::get_login_token(uid);
        assert_ne!(account_key, device);
        assert_eq!(resolve(&device, "Android").as_deref(), Some(account_key.as_str()));
        assert_eq!(userdata::uid_from_login_token(&device), uid);
        let sessions = list(uid, "");
        assert_eq!(sessions.len(), 1);
        assert_eq!(sessions[0]["platform"].as_str(), Some("Android"));
    }

    #[test]
    fn logging_out_everywhere_refuses_every_old_key() {
        let _lock = crate::runtime::lock_test_data_path();
        let device = global::create_token();
        let uid = userdata::get_acc(&device)["user"]["id"].as_i64().unwrap();
        let transferred = mint(uid, "game", "");
        let browser = mint(uid, "webui", "Firefox on Linux");
        let old_account_key = userdata::get_login_token(uid);
        assert!(resolve(&transferred, "").is_some());
        assert!(resolve(&old_account_key, "").is_some());

        assert!(revoke_all(uid, &browser));

        for token in [&device, &transferred, &old_account_key] {
            assert_eq!(resolve(token, ""), None);
            assert_eq!(userdata::uid_from_login_token(token), 0);
        }
        // A signed out key never falls through to the new account path
        let before = account_count();
        userdata::get_key(&device);
        assert_eq!(account_count(), before);

        // The browser that asked survives, and the account key moved on
        assert_eq!(webui_user(&browser), Some(uid));
        let new_account_key = userdata::get_login_token(uid);
        assert_ne!(new_account_key, old_account_key);
        assert_eq!(resolve(&new_account_key, "").as_deref(), Some(new_account_key.as_str()));
    }

    #[test]
    fn sessions_do_not_cross_between_web_and_game() {
        let _lock = crate::runtime::lock_test_data_path();
        let device = global::create_token();
        let uid = userdata::get_acc(&device)["user"]["id"].as_i64().unwrap();
        let browser = mint(uid, "webui", "");

        assert_eq!(resolve(&browser, ""), None);
        assert_eq!(userdata::uid_from_login_token(&browser), 0);
        assert_eq!(webui_user(&device), None);
        assert_eq!(webui_user(&browser), Some(uid));
    }

    #[test]
    fn one_session_can_be_revoked_alone() {
        let _lock = crate::runtime::lock_test_data_path();
        let phone = global::create_token();
        let uid = userdata::get_acc(&phone)["user"]["id"].as_i64().unwrap();
        let tablet = mint(uid, "game", "");
        resolve(&phone, "Android");
        resolve(&tablet, "iOS");

        let tablet_id = list(uid, "").members().find(|s| s["platform"] == "iOS").unwrap()["id"].as_i64().unwrap();
        assert!(!revoke(uid + 1, tablet_id));
        assert!(revoke(uid, tablet_id));
        assert!(!revoke(uid, tablet_id));

        assert_eq!(resolve(&tablet, ""), None);
        assert!(resolve(&phone, "").is_some());
        assert_eq!(list(uid, "").len(), 1);
    }
}

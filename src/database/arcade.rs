use lazy_static::lazy_static;
use rusqlite::params;
use jzon::{array, object, JsonValue};
use rand::RngExt;

use crate::router::global;
use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("arcade.db", setup_tables);
}

// A cabinet owns exactly two accounts forever: the MACHINE account (the identity
// the attract loop and its demo lives run on) and one reusable GUEST account,
// rewritten from scratch at every credit. Neither is ever handed out twice and
// neither accumulates, so the arcade never leaves dead users behind.
//
// Which account a card names lives in userdata.db (userdata::user::migration,
// the `cards` table): linking a card is a plain account feature and works with
// this module off. What lives here is only the cabinet side of a card:
// `card_sessions` is the "which cabinet is this card sitting at" record -
// `last_machine_id` / `last_session` attribute a card account's play to the
// machine that most recently ran a session for it - and `session_until` is the
// one that costs money: the moment the credit that /api/arcade/session took
// stops buying LP-free lives. Before it, a card account plays as a cabinet does;
// after it, the same account is an ordinary phone account again. It is a stamp
// rather than a flag so that a cabinet that loses power mid-credit expires on
// its own with nothing to clean up, and so an operator can size the window with
// --arcade-session-ttl. Re-linking a card clears its row (card_relinked).
//
// `plays` is the bookkeeping ledger: one row per arcade song, cleared or not.
// `cleared` is 0 for a song whose life gauge emptied: the client plays it out and
// reports it at /live/retire rather than /live/end (there is no cleared flag on
// the end wire and live_end_ex records a clear unconditionally), so the retire is
// the only place the ledger can learn about a failed song. A credit's play is two
// songs whether they were passed or not, which is what makes the total the number
// an operator's bookkeeping counts.
fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS machines (
    machine_id       TEXT NOT NULL PRIMARY KEY,
    name             TEXT NOT NULL,
    machine_user_id  BIGINT NOT NULL,
    guest_user_id    BIGINT NOT NULL,
    created          BIGINT NOT NULL,
    last_seen        BIGINT NOT NULL
);
CREATE TABLE IF NOT EXISTS card_sessions (
    card_id          TEXT NOT NULL PRIMARY KEY,
    last_machine_id  TEXT NOT NULL DEFAULT '',
    last_session     BIGINT NOT NULL DEFAULT 0,
    session_until    BIGINT NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS plays (
    id          INTEGER PRIMARY KEY,
    machine_id  TEXT NOT NULL,
    user_id     BIGINT NOT NULL,
    live_id     BIGINT NOT NULL,
    level       INT NOT NULL,
    score       BIGINT NOT NULL,
    rank        INT NOT NULL,
    at          BIGINT NOT NULL,
    cleared     INTEGER NOT NULL DEFAULT 1
);
CREATE INDEX IF NOT EXISTS plays_machine ON plays (machine_id);
    ").unwrap();
    // Databases from before the card mapping moved to userdata.db carry a
    // `cards` table here. Its mappings move over once, its windows become
    // card_sessions rows, and the table goes.
    if conn.prepare("SELECT user_id FROM cards LIMIT 1;").is_ok() {
        println!("Moving arcade card mappings to userdata");
        let has_sessions = conn.prepare("SELECT session_until FROM cards LIMIT 1;").is_ok();
        let query = if has_sessions {
            "SELECT card_id, user_id, created, last_machine_id, last_session, session_until FROM cards"
        } else {
            "SELECT card_id, user_id, created, '', 0, 0 FROM cards"
        };
        let mut stmt = conn.prepare(query).unwrap();
        let rows: Vec<(String, i64, i64, String, i64, i64)> = stmt.query_map([], |row| Ok((
            row.get(0)?, row.get(1)?, row.get(2)?, row.get(3)?, row.get(4)?, row.get(5)?
        ))).unwrap().flatten().collect();
        drop(stmt);
        for (card_id, user_id, created, last_machine_id, last_session, session_until) in rows {
            crate::router::userdata::user::migration::import_card(&card_id, user_id, created);
            if !last_machine_id.is_empty() || session_until != 0 {
                conn.execute(
                    "INSERT OR REPLACE INTO card_sessions (card_id, last_machine_id, last_session, session_until) VALUES (?1, ?2, ?3, ?4)",
                    params!(card_id, last_machine_id, last_session, session_until)
                ).unwrap();
            }
        }
        conn.execute("DROP TABLE cards;", []).unwrap();
    }
}

pub fn generate_machine_id() -> String {
    const CHARSET: &[u8] = b"0123456789abcdef";
    let mut rng = rand::rng();
    let id: String = (0..16)
        .map(|_| CHARSET[rng.random_range(0..CHARSET.len())] as char)
        .collect();
    if machine_exists(&id) {
        return generate_machine_id();
    }
    id
}

pub fn machine_exists(machine_id: &str) -> bool {
    DATABASE.lock_and_select("SELECT machine_id FROM machines WHERE machine_id=?1", params!(machine_id)).is_ok()
}

pub fn insert_machine(machine_id: &str, name: &str, machine_user_id: i64, guest_user_id: i64) {
    let now = global::timestamp() as i64;
    DATABASE.lock_and_exec(
        "INSERT INTO machines (machine_id, name, machine_user_id, guest_user_id, created, last_seen) VALUES (?1, ?2, ?3, ?4, ?5, ?5)",
        params!(machine_id, name, machine_user_id, guest_user_id, now)
    );
}

fn machine_row(conn: &rusqlite::Connection, machine_id: &str) -> Option<JsonValue> {
    let mut stmt = conn.prepare("SELECT machine_id, name, machine_user_id, guest_user_id, created, last_seen FROM machines WHERE machine_id=?1").ok()?;
    stmt.query_row(params!(machine_id), |row| {
        Ok(object!{
            machine_id: row.get::<usize, String>(0)?,
            name: row.get::<usize, String>(1)?,
            machine_user_id: row.get::<usize, i64>(2)?,
            guest_user_id: row.get::<usize, i64>(3)?,
            created: row.get::<usize, i64>(4)?,
            last_seen: row.get::<usize, i64>(5)?
        })
    }).ok()
}

pub fn get_machine(machine_id: &str) -> Option<JsonValue> {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).ok()?;
    machine_row(&conn, machine_id)
}

// The cabinet is alive: every session (and every play it records) stamps it, and
// the purge sweeper measures a machine's age from here
pub fn touch_machine(machine_id: &str) {
    DATABASE.lock_and_exec("UPDATE machines SET last_seen=?1 WHERE machine_id=?2", params!(global::timestamp() as i64, machine_id));
}

// The machine a user account belongs to, when the account IS one of a cabinet's
// own two identities. A card-bound player account is nobody's, and answers None
pub fn machine_of_account(user_id: i64) -> Option<JsonValue> {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).ok()?;
    let mut stmt = conn.prepare("SELECT machine_id FROM machines WHERE machine_user_id=?1 OR guest_user_id=?1").ok()?;
    let machine_id: String = stmt.query_row(params!(user_id), |row| row.get(0)).ok()?;
    machine_row(&conn, &machine_id)
}

pub fn delete_machine(machine_id: &str) {
    DATABASE.lock_and_exec("DELETE FROM plays WHERE machine_id=?1", params!(machine_id));
    DATABASE.lock_and_exec("DELETE FROM card_sessions WHERE last_machine_id=?1", params!(machine_id));
    DATABASE.lock_and_exec("DELETE FROM machines WHERE machine_id=?1", params!(machine_id));
}

// Every machine with its play count, newest sighting first - the webui list
pub fn list_machines() -> JsonValue {
    let Ok(conn) = rusqlite::Connection::open(DATABASE.get_path()) else {
        return array![];
    };
    let Ok(mut stmt) = conn.prepare("
        SELECT m.machine_id, m.name, m.machine_user_id, m.guest_user_id, m.created, m.last_seen,
               (SELECT COUNT(*) FROM plays p WHERE p.machine_id = m.machine_id),
               (SELECT COUNT(*) FROM plays p WHERE p.machine_id = m.machine_id AND p.cleared <> 0)
        FROM machines m ORDER BY m.last_seen DESC
    ") else {
        return array![];
    };
    let Ok(mapped) = stmt.query_map(params!(), |row| {
        Ok(object!{
            machine_id: row.get::<usize, String>(0)?,
            name: row.get::<usize, String>(1)?,
            machine_user_id: row.get::<usize, i64>(2)?,
            guest_user_id: row.get::<usize, i64>(3)?,
            created: row.get::<usize, i64>(4)?,
            last_seen: row.get::<usize, i64>(5)?,
            play_count: row.get::<usize, i64>(6)?,
            cleared_count: row.get::<usize, i64>(7)?
        })
    }) else {
        return array![];
    };
    let mut rv = array![];
    for row in mapped.flatten() {
        rv.push(row).unwrap();
    }
    rv
}

// Machines whose last sighting is older than `cutoff`. The two account ids come
// back with them: the sweeper deletes the accounts in the same pass
pub fn machines_last_seen_before(cutoff: i64) -> JsonValue {
    let Ok(conn) = rusqlite::Connection::open(DATABASE.get_path()) else {
        return array![];
    };
    let Ok(mut stmt) = conn.prepare("SELECT machine_id, machine_user_id, guest_user_id, last_seen FROM machines WHERE last_seen < ?1") else {
        return array![];
    };
    let Ok(mapped) = stmt.query_map(params!(cutoff), |row| {
        Ok(object!{
            machine_id: row.get::<usize, String>(0)?,
            machine_user_id: row.get::<usize, i64>(1)?,
            guest_user_id: row.get::<usize, i64>(2)?,
            last_seen: row.get::<usize, i64>(3)?
        })
    }) else {
        return array![];
    };
    let mut rv = array![];
    for row in mapped.flatten() {
        rv.push(row).unwrap();
    }
    rv
}

// The cabinet this card is playing at right now and how long the credit it just
// paid buys LP-free lives for. Written by /api/arcade/session, and only there:
// the session is the one moment the server knows a credit was taken
pub fn open_card_session(card_id: &str, machine_id: &str, until: i64) {
    DATABASE.lock_and_exec(
        "INSERT INTO card_sessions (card_id, last_machine_id, last_session, session_until) VALUES (?1, ?2, ?3, ?4)
         ON CONFLICT(card_id) DO UPDATE SET last_machine_id=?2, last_session=?3, session_until=?4",
        params!(card_id, machine_id, global::timestamp() as i64, until)
    );
}

// A card that changed hands takes nothing of the previous holder's credit with it
pub fn clear_card_session(card_id: &str) {
    DATABASE.lock_and_exec("DELETE FROM card_sessions WHERE card_id=?1", params!(card_id));
}

fn placeholders(count: usize) -> String {
    (1..=count).map(|i| format!("?{}", i)).collect::<Vec<_>>().join(", ")
}

// Of these cards (an account's, userdata::user::migration::cards_of_account),
// the one whose cabinet session is still open at `now`, as (card id, when the
// session started). None when none of them is at a cabinet - which is every
// phone account, and every card between credits.
pub fn live_card_session(cards: &[String], now: i64) -> Option<(String, i64)> {
    if cards.is_empty() {
        return None;
    }
    let conn = rusqlite::Connection::open(DATABASE.get_path()).ok()?;
    let sql = format!(
        "SELECT card_id, last_session FROM card_sessions WHERE card_id IN ({}) AND session_until>?{} ORDER BY session_until DESC LIMIT 1",
        placeholders(cards.len()), cards.len() + 1
    );
    let mut stmt = conn.prepare(&sql).ok()?;
    let mut args: Vec<&dyn rusqlite::ToSql> = cards.iter().map(|c| c as &dyn rusqlite::ToSql).collect();
    args.push(&now);
    stmt.query_row(args.as_slice(), |row| Ok((row.get::<usize, String>(0)?, row.get::<usize, i64>(1)?))).ok()
}

// A live starting inside the window pushes its end back, so a credit whose songs
// run long is not cut off mid-play. Only ever forward, and never past the
// ceiling the caller computed from the session itself: extending without a
// ceiling would turn one credit into an endless supply of LP-free lives
pub fn extend_card_session(card_id: &str, until: i64) {
    DATABASE.lock_and_exec(
        "UPDATE card_sessions SET session_until=?1 WHERE card_id=?2 AND session_until<?1",
        params!(until, card_id)
    );
}

// Only the session tests need to age a card's window; production code only ever
// opens one at a session or pushes it forward through extend_card_session
#[cfg(test)]
pub fn backdate_card_session_for_test(card_id: &str, last_session: i64, session_until: i64) {
    DATABASE.lock_and_exec(
        "UPDATE card_sessions SET last_session=?1, session_until=?2 WHERE card_id=?3",
        params!(last_session, session_until, card_id)
    );
}

// The machine that most recently ran a session for any of these cards. None
// when none of them has ever been at a cabinet
pub fn last_machine_of_cards(cards: &[String]) -> Option<String> {
    if cards.is_empty() {
        return None;
    }
    let conn = rusqlite::Connection::open(DATABASE.get_path()).ok()?;
    let sql = format!(
        "SELECT last_machine_id FROM card_sessions WHERE card_id IN ({}) AND last_machine_id<>'' ORDER BY last_session DESC LIMIT 1",
        placeholders(cards.len())
    );
    let mut stmt = conn.prepare(&sql).ok()?;
    let args: Vec<&dyn rusqlite::ToSql> = cards.iter().map(|c| c as &dyn rusqlite::ToSql).collect();
    let machine_id: String = stmt.query_row(args.as_slice(), |row| row.get(0)).ok()?;
    if machine_id.is_empty() {
        return None;
    }
    Some(machine_id)
}

pub fn insert_play(machine_id: &str, user_id: i64, live_id: i64, level: i64, score: i64, rank: i64, cleared: bool) {
    DATABASE.lock_and_exec(
        "INSERT INTO plays (machine_id, user_id, live_id, level, score, rank, at, cleared) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
        params!(machine_id, user_id, live_id, level, score, rank, global::timestamp() as i64, i64::from(cleared))
    );
}

// Only the sweeper's own test needs to move a cabinet's clock; production code
// only ever stamps last_seen forward through touch_machine
#[cfg(test)]
pub fn backdate_machine_for_test(machine_id: &str, last_seen: i64) {
    DATABASE.lock_and_exec("UPDATE machines SET last_seen=?1 WHERE machine_id=?2", params!(last_seen, machine_id));
}

pub fn play_count(machine_id: &str) -> i64 {
    DATABASE.lock_and_select_type("SELECT COUNT(*) FROM plays WHERE machine_id=?1", params!(machine_id)).unwrap_or(0)
}

// The cabinet's ledger, newest first. Only the tests read it back today - the
// webui counts rows rather than listing them - but the ledger exists to be read
pub fn plays_of_machine(machine_id: &str) -> JsonValue {
    let Ok(conn) = rusqlite::Connection::open(DATABASE.get_path()) else {
        return array![];
    };
    let Ok(mut stmt) = conn.prepare(
        "SELECT user_id, live_id, level, score, rank, at, cleared FROM plays WHERE machine_id=?1 ORDER BY id DESC"
    ) else {
        return array![];
    };
    let Ok(mapped) = stmt.query_map(params!(machine_id), |row| {
        Ok(object!{
            user_id: row.get::<usize, i64>(0)?,
            live_id: row.get::<usize, i64>(1)?,
            level: row.get::<usize, i64>(2)?,
            score: row.get::<usize, i64>(3)?,
            rank: row.get::<usize, i64>(4)?,
            at: row.get::<usize, i64>(5)?,
            cleared: row.get::<usize, i64>(6)? != 0
        })
    }) else {
        return array![];
    };
    let mut rv = array![];
    for row in mapped.flatten() {
        rv.push(row).unwrap();
    }
    rv
}

#[cfg(test)]
mod tests {
    use super::*;

    // A cabinet owns its two accounts, is found from either of them, and its
    // last_seen is what the purge sweeper measures
    #[test]
    fn a_machine_owns_its_two_accounts() {
        let _lock = crate::runtime::lock_test_data_path();

        let id = generate_machine_id();
        assert_eq!(id.len(), 16);
        assert!(id.chars().all(|c| c.is_ascii_hexdigit()));

        insert_machine(&id, "Cabinet 1", 111_111_111_111_111, 222_222_222_222_222);
        let machine = get_machine(&id).unwrap();
        assert_eq!(machine["name"].as_str(), Some("Cabinet 1"));
        assert_eq!(machine["machine_user_id"].as_i64(), Some(111_111_111_111_111));
        assert_eq!(machine["guest_user_id"].as_i64(), Some(222_222_222_222_222));

        // Either of the two identities finds the cabinet; a stranger does not
        assert_eq!(machine_of_account(111_111_111_111_111).unwrap()["machine_id"].as_str(), Some(id.as_str()));
        assert_eq!(machine_of_account(222_222_222_222_222).unwrap()["machine_id"].as_str(), Some(id.as_str()));
        assert!(machine_of_account(999_999_999_999_999).is_none());

        // Aged out by the sweeper's cutoff, and only then
        let seen = machine["last_seen"].as_i64().unwrap();
        assert!(machines_last_seen_before(seen + 1).members().any(|m| m["machine_id"] == id.as_str()));
        assert!(!machines_last_seen_before(seen).members().any(|m| m["machine_id"] == id.as_str()));

        delete_machine(&id);
        assert!(get_machine(&id).is_none());
    }

    // A card's cabinet record follows the card: it is attributed to the cabinet
    // the card last sat at, a re-link clears it, and it dies with the machine
    #[test]
    fn a_cards_cabinet_record_follows_the_card() {
        let _lock = crate::runtime::lock_test_data_path();

        let machine = generate_machine_id();
        insert_machine(&machine, "Cabinet 2", 333_333_333_333_333, 444_444_444_444_444);
        let cards = vec!["0123456789012345".to_string()];
        let card = cards[0].as_str();

        // Never at a cabinet yet
        assert!(last_machine_of_cards(&cards).is_none());
        assert!(live_card_session(&[], 0).is_none());

        let now = global::timestamp() as i64;
        open_card_session(card, &machine, now + 60);
        assert_eq!(last_machine_of_cards(&cards).as_deref(), Some(machine.as_str()));

        // A re-link forgets the cabinet and the window the previous account's credit paid for
        clear_card_session(card);
        assert!(last_machine_of_cards(&cards).is_none());
        assert!(live_card_session(&cards, now).is_none(), "a re-link carried the previous account's credit over");

        assert_eq!(play_count(&machine), 0);
        insert_play(&machine, 666_666_666_666_666, 1100101, 4, 654_321, 3, true);
        insert_play(&machine, 666_666_666_666_666, 1100101, 4, 123_456, 2, false);
        assert_eq!(play_count(&machine), 2);
        // A failed song is a song: it is in the ledger and in the total, and the
        // cleared count is what separates the two
        assert!(list_machines().members().any(|m|
            m["machine_id"] == machine.as_str() && m["play_count"] == 2 && m["cleared_count"] == 1
        ));
        let ledger = plays_of_machine(&machine);
        assert_eq!(ledger.len(), 2);
        assert_eq!(ledger[0]["cleared"].as_bool(), Some(false));
        assert_eq!(ledger[0]["score"].as_i64(), Some(123_456));
        assert_eq!(ledger[1]["cleared"].as_bool(), Some(true));

        // Removing the cabinet takes its ledger and its card records with it
        open_card_session(card, &machine, now + 60);
        delete_machine(&machine);
        assert_eq!(play_count(&machine), 0);
        assert!(last_machine_of_cards(&cards).is_none());
        assert!(live_card_session(&cards, now).is_none(), "a retired cabinet left a credit open");
    }

    // The credit a card paid for is a window on its own row: open until it is
    // not, pushed forward but never backward, and never shared with another card
    #[test]
    fn a_credit_opens_a_window_that_closes_on_its_own() {
        let _lock = crate::runtime::lock_test_data_path();

        let machine = generate_machine_id();
        insert_machine(&machine, "Cabinet 3", 777_777_777_777_777, 888_888_888_888_888);
        let cards = vec!["1212121212121212".to_string()];
        let card = cards[0].as_str();
        let now = global::timestamp() as i64;

        // A card with no session behind it is not a cabinet session
        assert!(live_card_session(&cards, now).is_none());

        open_card_session(card, &machine, now + 600);
        let (open_card, opened) = live_card_session(&cards, now).expect("the credit did not open a window");
        assert_eq!(open_card, card);
        assert!(opened <= now && opened >= now - 5, "the session start was not stamped: {} vs {}", opened, now);

        // The window closes by itself, with nothing to sweep
        assert!(live_card_session(&cards, now + 599).is_some());
        assert!(live_card_session(&cards, now + 600).is_none(), "the window outlived its own expiry");
        assert!(live_card_session(&cards, now + 601).is_none());

        // A live inside it pushes it forward, and only forward
        extend_card_session(card, now + 1200);
        assert!(live_card_session(&cards, now + 900).is_some());
        extend_card_session(card, now + 300);
        assert!(live_card_session(&cards, now + 900).is_some(), "an extension moved the window backwards");

        // Another card is untouched by any of it
        let other = vec!["3434343434343434".to_string()];
        assert!(live_card_session(&other, now).is_none());

        delete_machine(&machine);
    }
}

use lazy_static::lazy_static;
use rusqlite::params;
use jzon::{array, object, JsonValue};

use crate::router::global;
use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("announcements.db", setup_tables);
}

// 1 = notice, 2 = update, 3 = bug
pub const CATEGORIES: &[i64] = &[1, 2, 3];

pub const TYPES: &[&str] = &["news", "event", "gacha", "maintenance", "shop", "others"];

pub fn is_valid_category(category: i64) -> bool {
    CATEGORIES.contains(&category)
}

pub fn is_valid_type(kind: &str) -> bool {
    TYPES.contains(&kind)
}

fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS announcements (
    id            INTEGER PRIMARY KEY AUTOINCREMENT,
    category      INTEGER NOT NULL,
    type          TEXT NOT NULL,
    title         TEXT NOT NULL,
    body          TEXT NOT NULL,
    banner        BLOB,
    updated       INTEGER NOT NULL DEFAULT 0,
    visible       INTEGER NOT NULL DEFAULT 1,
    published_at  BIGINT NOT NULL,
    created_by    BIGINT NOT NULL,
    created_at    BIGINT NOT NULL
);
    ").unwrap();
}

pub enum Banner {
    Keep,
    Clear,
    Set(Vec<u8>)
}

fn row_to_json(row: &rusqlite::Row) -> rusqlite::Result<JsonValue> {
    Ok(object!{
        id: row.get::<usize, i64>(0)?,
        category: row.get::<usize, i64>(1)?,
        type: row.get::<usize, String>(2)?,
        title: row.get::<usize, String>(3)?,
        body: row.get::<usize, String>(4)?,
        has_banner: row.get::<usize, i64>(5)? != 0,
        updated: row.get::<usize, i64>(6)? != 0,
        visible: row.get::<usize, i64>(7)? != 0,
        published_at: row.get::<usize, i64>(8)?,
        created_by: row.get::<usize, i64>(9)?,
        created_at: row.get::<usize, i64>(10)?
    })
}

const COLUMNS: &str = "id, category, type, title, body, banner IS NOT NULL, updated, visible, published_at, created_by, created_at";

fn query(where_clause: &str, args: &[&dyn rusqlite::ToSql]) -> JsonValue {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    let sql = format!("SELECT {COLUMNS} FROM announcements {where_clause} ORDER BY published_at DESC, id DESC");
    let Ok(mut stmt) = conn.prepare(&sql) else {
        return array![];
    };
    let Ok(mapped) = stmt.query_map(args, |row| row_to_json(row)) else {
        return array![];
    };
    let mut rv = array![];
    for row in mapped.flatten() {
        rv.push(row).unwrap();
    }
    rv
}

pub fn list_category(category: i64) -> JsonValue {
    query("WHERE visible=1 AND category=?1", params!(category))
}

pub fn get_all() -> JsonValue {
    query("", params!())
}

pub fn get(id: i64) -> Option<JsonValue> {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    let sql = format!("SELECT {COLUMNS} FROM announcements WHERE id=?1");
    conn.query_row(&sql, params!(id), |row| row_to_json(row)).ok()
}

pub fn get_banner(id: i64) -> Option<Vec<u8>> {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    conn.query_row("SELECT banner FROM announcements WHERE id=?1 AND banner IS NOT NULL", params!(id), |row| row.get::<usize, Vec<u8>>(0)).ok()
}

pub fn get_public_banner(id: i64) -> Option<Vec<u8>> {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    conn.query_row("SELECT banner FROM announcements WHERE id=?1 AND visible=1 AND banner IS NOT NULL", params!(id), |row| row.get::<usize, Vec<u8>>(0)).ok()
}

pub fn visible_ids(category: Option<i64>) -> Vec<i64> {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    let (sql, args): (&str, Vec<&dyn rusqlite::ToSql>) = match &category {
        Some(c) => ("SELECT id FROM announcements WHERE visible=1 AND category=?1", vec![c]),
        None => ("SELECT id FROM announcements WHERE visible=1", vec![])
    };
    let Ok(mut stmt) = conn.prepare(sql) else {
        return Vec::new();
    };
    let Ok(mapped) = stmt.query_map(args.as_slice(), |row| row.get::<usize, i64>(0)) else {
        return Vec::new();
    };
    mapped.flatten().collect()
}

pub fn latest_published_at() -> i64 {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    conn.query_row("SELECT MAX(published_at) FROM announcements WHERE visible=1", params!(), |row| row.get::<usize, i64>(0)).unwrap_or(0)
}

pub fn create(category: i64, kind: &str, title: &str, body: &str, banner: Option<Vec<u8>>, updated: bool, visible: bool, published_at: i64, created_by: i64) -> i64 {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    conn.execute(
        "INSERT INTO announcements (category, type, title, body, banner, updated, visible, published_at, created_by, created_at) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
        params!(category, kind, title, body, banner, updated as i64, visible as i64, published_at, created_by, global::timestamp() as i64)
    ).unwrap();
    conn.last_insert_rowid()
}

pub fn update(id: i64, category: i64, kind: &str, title: &str, body: &str, banner: Banner, updated: bool, visible: bool, published_at: i64) {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    conn.execute(
        "UPDATE announcements SET category=?2, type=?3, title=?4, body=?5, updated=?6, visible=?7, published_at=?8 WHERE id=?1",
        params!(id, category, kind, title, body, updated as i64, visible as i64, published_at)
    ).unwrap();
    match banner {
        Banner::Keep => {},
        Banner::Clear => { conn.execute("UPDATE announcements SET banner=NULL WHERE id=?1", params!(id)).unwrap(); },
        Banner::Set(bytes) => { conn.execute("UPDATE announcements SET banner=?2 WHERE id=?1", params!(id, bytes)).unwrap(); }
    }
}

pub fn delete(id: i64) {
    DATABASE.lock_and_exec("DELETE FROM announcements WHERE id=?1", params!(id));
}




/// more tests??? This is probably a good thing but man haha

#[cfg(test)]
mod tests {
    use super::*;

    fn wipe() {
        DATABASE.lock_and_exec("DELETE FROM announcements", params!());
    }

    #[test]
    fn create_list_and_category_filter() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe();

        let a = create(1, "news", "First", "<p>body</p>", None, false, true, 1000, 42);
        let b = create(1, "event", "Second", "body", Some(vec![1, 2, 3]), true, true, 2000, 42);
        let c = create(2, "gacha", "Other tab", "body", None, false, true, 1500, 42);
        let hidden = create(1, "news", "Draft", "body", None, false, false, 3000, 42);

        // One tab, visible only, newest first
        let cat1 = list_category(1);
        assert_eq!(cat1.len(), 2);
        assert_eq!(cat1[0]["id"].as_i64(), Some(b));
        assert_eq!(cat1[1]["id"].as_i64(), Some(a));
        assert!(cat1.members().all(|row| row["id"].as_i64() != Some(hidden)));

        // has_banner reflects the blob without carrying it
        assert_eq!(cat1[0]["has_banner"].as_bool(), Some(true));
        assert_eq!(cat1[1]["has_banner"].as_bool(), Some(false));
        assert_eq!(get_banner(b), Some(vec![1, 2, 3]));
        assert_eq!(get_banner(a), None);

        // The other tab is untouched, the admin view sees the draft too
        assert_eq!(list_category(2).len(), 1);
        assert_eq!(list_category(2)[0]["id"].as_i64(), Some(c));
        assert_eq!(get_all().len(), 4);

        assert_eq!(latest_published_at(), 2000);
        let mut visible = visible_ids(None);
        visible.sort();
        assert_eq!(visible, vec![a, b, c]);
        let mut cat1_ids = visible_ids(Some(1));
        cat1_ids.sort();
        assert_eq!(cat1_ids, vec![a, b]);

        wipe();
    }

    #[test]
    fn update_rewrites_and_banner_transitions() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe();

        let id = create(1, "news", "Title", "body", Some(vec![9]), false, true, 1000, 7);
        update(id, 3, "maintenance", "New title", "new body", Banner::Keep, true, true, 5000);
        let row = get(id).unwrap();
        assert_eq!(row["category"].as_i64(), Some(3));
        assert_eq!(row["type"].as_str(), Some("maintenance"));
        assert_eq!(row["title"].as_str(), Some("New title"));
        assert_eq!(row["updated"].as_bool(), Some(true));
        assert_eq!(row["published_at"].as_i64(), Some(5000));
        // Keep left the blob in place
        assert_eq!(get_banner(id), Some(vec![9]));

        update(id, 3, "maintenance", "New title", "new body", Banner::Set(vec![4, 5]), true, true, 5000);
        assert_eq!(get_banner(id), Some(vec![4, 5]));
        update(id, 3, "maintenance", "New title", "new body", Banner::Clear, true, false, 5000);
        assert_eq!(get_banner(id), None);
        assert_eq!(get(id).unwrap()["visible"].as_bool(), Some(false));

        delete(id);
        assert!(get(id).is_none());
        wipe();
    }

    // Ids are sequential, so the player-facing banner route is pollable: a
    // draft's banner must be reachable by the admin lookup and by nothing else
    #[test]
    fn a_drafts_banner_is_admin_only() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe();

        let published = create(1, "news", "Live", "body", Some(vec![1, 2]), false, true, 1000, 42);
        let draft = create(1, "news", "Unannounced", "body", Some(vec![7, 7]), false, false, 2000, 42);

        assert_eq!(get_banner(draft), Some(vec![7, 7]));
        assert_eq!(get_public_banner(draft), None);
        assert_eq!(get_public_banner(published), Some(vec![1, 2]));

        // Publishing it makes the banner reachable, hiding it again takes it back
        update(draft, 1, "news", "Unannounced", "body", Banner::Keep, false, true, 2000);
        assert_eq!(get_public_banner(draft), Some(vec![7, 7]));
        update(draft, 1, "news", "Unannounced", "body", Banner::Keep, false, false, 2000);
        assert_eq!(get_public_banner(draft), None);

        wipe();
    }

    #[test]
    fn vocabulary_is_wellformed() {
        for category in CATEGORIES {
            assert!(is_valid_category(*category));
        }
        assert!(!is_valid_category(0));
        assert!(!is_valid_category(4));
        for kind in TYPES {
            assert!(is_valid_type(kind));
        }
        assert!(!is_valid_type("explosion"));
    }
}

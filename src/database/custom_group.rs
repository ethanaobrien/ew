use lazy_static::lazy_static;
use rusqlite::params;
use jzon::{array, object, JsonValue};
use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("custom_groups.db", setup_tables);
}

pub const FIRST_ID: i64 = 10_000;

fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("CREATE TABLE IF NOT EXISTS groups (
        id INTEGER PRIMARY KEY, name TEXT NOT NULL, name_en TEXT NOT NULL
    );
    CREATE UNIQUE INDEX IF NOT EXISTS custom_groups_unique_name ON groups(name COLLATE NOCASE);
    CREATE TABLE IF NOT EXISTS logos (group_id INTEGER PRIMARY KEY, md5 TEXT NOT NULL, size INTEGER NOT NULL);
    CREATE TABLE IF NOT EXISTS sequence (id INTEGER PRIMARY KEY CHECK (id=1), last_id INTEGER NOT NULL);").unwrap();
}

pub fn list() -> JsonValue {
    let conn = rusqlite::Connection::open(DATABASE.get_path()).unwrap();
    let Ok(mut stmt) = conn.prepare("SELECT id, name, name_en, COALESCE(md5, ''), COALESCE(size, 0) FROM groups LEFT JOIN logos ON group_id=id ORDER BY id") else { return array![]; };
    let Ok(rows) = stmt.query_map(params![], |row| Ok(object! {
        "id": row.get::<_, i64>(0)?,
        "name": row.get::<_, String>(1)?,
        "name_en": row.get::<_, String>(2)?,
        "logo_md5": row.get::<_, String>(3)?,
        "logo_size": row.get::<_, i64>(4)?
    })) else { return array![]; };
    let mut result = array![];
    for row in rows.flatten() { result.push(row).unwrap(); }
    result
}

pub fn set_logo(id: i64, md5: &str, size: i64) -> Result<(), rusqlite::Error> {
    DATABASE.lock_and_transact(|conn| {
        conn.execute("INSERT INTO logos (group_id, md5, size) VALUES (?1, ?2, ?3) ON CONFLICT(group_id) DO UPDATE SET md5=excluded.md5, size=excluded.size", params!(id, md5, size))?;
        Ok(())
    })
}

pub fn has_logo(md5: &str) -> bool {
    DATABASE.lock_and_select("SELECT group_id FROM logos WHERE md5=?1", params!(md5)).is_ok()
}

pub fn exists(id: i64) -> bool {
    if id < FIRST_ID { return false; }
    DATABASE.lock_and_select("SELECT id FROM groups WHERE id=?1", params!(id)).is_ok()
}

pub fn create(name: &str, name_en: &str) -> Result<i64, rusqlite::Error> {
    DATABASE.lock_and_transact(|conn| {
        let last: i64 = conn.query_row("SELECT last_id FROM sequence WHERE id=1", [], |row| row.get(0)).unwrap_or(FIRST_ID - 1);
        let id = last + 1;
        conn.execute("INSERT INTO groups (id, name, name_en) VALUES (?1, ?2, ?3)", params!(id, name, name_en))?;
        conn.execute("INSERT INTO sequence (id, last_id) VALUES (1, ?1) ON CONFLICT(id) DO UPDATE SET last_id=?1", params!(id))?;
        Ok(id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adds_logo_storage_to_existing_group_database() {
        let conn = rusqlite::Connection::open_in_memory().unwrap();
        conn.execute_batch("CREATE TABLE groups (id INTEGER PRIMARY KEY, name TEXT NOT NULL, name_en TEXT NOT NULL);
            INSERT INTO groups VALUES (10000, 'Team A', 'Team A');").unwrap();
        setup_tables(&conn);
        setup_tables(&conn);
        assert_eq!(conn.query_row("SELECT name FROM groups WHERE id=10000", [], |row| row.get::<_, String>(0)).unwrap(), "Team A");
        conn.execute("INSERT INTO logos VALUES (10000, 'test-hash', 42)", []).unwrap();
    }
}

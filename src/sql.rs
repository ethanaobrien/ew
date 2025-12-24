use rusqlite::{Connection, ToSql};
use json::{JsonValue, array};

pub struct SQLite {
    path: String
}

impl SQLite {
    pub fn new(path: &str, setup: fn(&Connection)) -> SQLite {
        let instance = SQLite {
            path: crate::get_data_path(path)
        };
        let conn = Connection::open(&instance.path).unwrap();
        conn.busy_timeout(std::time::Duration::from_secs(10)).unwrap();
        conn.execute("PRAGMA foreign_keys = ON;", ()).unwrap();
        setup(&conn);
        instance
    }
    pub fn get_path(&self) -> &str {
        &self.path
    }
    pub fn lock_and_exec(&self, command: &str, args: &[&dyn ToSql]) {
        let conn = Connection::open(&self.path).unwrap();
        conn.execute(command, args).unwrap();
    }
    pub fn lock_and_select(&self, command: &str, args: &[&dyn ToSql]) -> Result<String, rusqlite::Error> {
        let conn = Connection::open(&self.path).unwrap();
        let mut stmt = conn.prepare(command)?;
        stmt.query_row(args, |row| {
            match row.get::<usize, i64>(0) {
                Ok(val) => Ok(val.to_string()),
                Err(_) => row.get(0)
            }
        })
    }
    pub fn lock_and_select_type<T: rusqlite::types::FromSql>(&self, command: &str, args: &[&dyn ToSql]) -> Result<T, rusqlite::Error> {
        let conn = Connection::open(&self.path).unwrap();
        let mut stmt = conn.prepare(command)?;
        stmt.query_row(args, |row| {
            row.get(0)
        })
    }
    pub fn lock_and_select_all(&self, command: &str, args: &[&dyn ToSql]) -> Result<JsonValue, rusqlite::Error> {
        let conn = Connection::open(&self.path).unwrap();
        let mut stmt = conn.prepare(command)?;
        let map = stmt.query_map(args, |row| {
            match row.get::<usize, i64>(0) {
                Ok(val) => Ok(val.to_string()),
                Err(_) => row.get(0)
            }
        })?;
        let mut rv = array![];
        for val in map {
            let res = val?;
            match res.clone().parse::<i64>() {
                Ok(v) => rv.push(v).unwrap(),
                Err(_) => rv.push(res).unwrap()
            };
        }
        Ok(rv)
    }
}

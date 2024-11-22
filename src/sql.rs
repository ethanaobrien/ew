use rusqlite::{Connection, params, ToSql};
use std::sync::Mutex;
use json::{JsonValue, array};

use crate::router::clear_rate::Live;

macro_rules! lock_onto_mutex {
    ($mutex:expr) => {{
        loop {
            match $mutex.lock() {
                Ok(value) => {
                    break value;
                }
                Err(_) => {
                    $mutex.clear_poison();
                    std::thread::sleep(std::time::Duration::from_millis(10));
                }
            }
        }
    }};
}

pub struct SQLite {
    engine: Mutex<Connection>
}

impl SQLite {
    pub fn new(path: &str, setup: fn(&SQLite)) -> SQLite {
        let conn = Connection::open(crate::get_data_path(path)).unwrap();
        conn.execute("PRAGMA foreign_keys = ON;", ()).unwrap();
        let instance = SQLite {
            engine: Mutex::new(conn)
        };
        setup(&instance);
        instance
    }
    pub fn lock_and_exec(&self, command: &str, args: &[&dyn ToSql]) {
        let conn = lock_onto_mutex!(self.engine);
        conn.execute(command, args).unwrap();
    }
    pub fn lock_and_select(&self, command: &str, args: &[&dyn ToSql]) -> Result<String, rusqlite::Error> {
        let conn = lock_onto_mutex!(self.engine);
        let mut stmt = conn.prepare(command)?;
        stmt.query_row(args, |row| {
            match row.get::<usize, i64>(0) {
                Ok(val) => Ok(val.to_string()),
                Err(_) => row.get(0)
            }
        })
    }
    pub fn lock_and_select_all(&self, command: &str, args: &[&dyn ToSql]) -> Result<JsonValue, rusqlite::Error> {
        let conn = lock_onto_mutex!(self.engine);
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
    pub fn get_live_data(&self, id: i64) -> Result<Live, rusqlite::Error> {
        let conn = lock_onto_mutex!(self.engine);
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
    pub fn create_store_v2(&self, table: &str) {
        self.lock_and_exec(table, params!());
    }
}

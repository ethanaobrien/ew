use rusqlite::{Connection, params, ToSql};
use std::sync::Mutex;
use json::{JsonValue, array};
use std::fs;

use crate::router::clear_rate::Live;

pub struct SQLite {
    engine: Mutex<Connection>,
    sleep_duration: u64
}

impl SQLite {
    pub fn new(path: &str, setup: fn(&SQLite)) -> SQLite {
        let args = crate::get_args();
        fs::create_dir_all(&args.path).unwrap();
        let conn = Connection::open(format!("{}/{}", args.path, path)).unwrap();
        conn.execute("PRAGMA foreign_keys = ON;", ()).unwrap();
        let instance = SQLite {
            engine: Mutex::new(conn),
            sleep_duration: 10
        };
        setup(&instance);
        instance
    }
    pub fn lock_and_exec(&self, command: &str, args: &[&dyn ToSql]) {
        loop {
            match self.engine.lock() {
                Ok(conn) => {
                    conn.execute(command, args).unwrap();
                    return;
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(self.sleep_duration));
                }
            }
        }
    }
    pub fn lock_and_select(&self, command: &str, args: &[&dyn ToSql]) -> Result<String, rusqlite::Error> {
        loop {
            match self.engine.lock() {
                Ok(conn) => {
                    let mut stmt = conn.prepare(command)?;
                    return stmt.query_row(args, |row| {
                        match row.get::<usize, i64>(0) {
                            Ok(val) => Ok(val.to_string()),
                            Err(_) => row.get(0)
                        }
                    });
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(self.sleep_duration));
                }
            }
        }
    }
    pub fn lock_and_select_all(&self, command: &str, args: &[&dyn ToSql]) -> Result<JsonValue, rusqlite::Error> {
        loop {
            match self.engine.lock() {
                Ok(conn) => {
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
                    return Ok(rv);
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(self.sleep_duration));
                }
            }
        }
    }
    pub fn get_live_data(&self, id: i64) -> Result<Live, rusqlite::Error> {
        loop {
            match self.engine.lock() {
                Ok(conn) => {
                    let mut stmt = conn.prepare("SELECT * FROM lives WHERE live_id=?1")?;
                    return stmt.query_row(params!(id), |row| {
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
                    });
                }
                Err(_) => {
                    std::thread::sleep(std::time::Duration::from_millis(self.sleep_duration));
                }
            }
        }
    }
    pub fn create_store_v2(&self, table: &str) {
        self.lock_and_exec(table, params!());
    }
}

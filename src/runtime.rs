use lazy_static::lazy_static;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, RwLock};
use std::fs;

lazy_static! {
    static ref RUNNING: RwLock<bool> = RwLock::new(false);
    static ref DATAPATH: RwLock<String> = RwLock::new(String::new());
    static ref MASTERDATA_PATH: RwLock<String> = RwLock::new(String::new());
    static ref MASTERDATA_WARNED: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
    static ref EASTER: RwLock<bool> = RwLock::new(false);
}

pub fn set_running(running: bool) {
    let mut w = RUNNING.write().unwrap();
    *w = running;
}

pub fn get_running() -> bool {
    *RUNNING.read().unwrap()
}

pub fn get_data_path(file_name: &str) -> String {
    let mut path = {
        DATAPATH.read().unwrap().clone()
    };
    while path.ends_with('/') {
        path.pop();
    }
    fs::create_dir_all(&path).unwrap();
    format!("{}/{}", path, file_name)
}

pub fn update_data_path(path: &str) {
    let mut w = DATAPATH.write().unwrap();
    *w = path.to_string();
}

pub fn update_masterdata_path(path: &str) {
    let trimmed = path.trim_end_matches('/').to_string();
    let mut w = MASTERDATA_PATH.write().unwrap();
    if trimmed.is_empty() {
        *w = String::new();
        return;
    }
    if !Path::new(&trimmed).is_dir() {
        println!("Couldn't find masterdata directory {}", trimmed);
        *w = String::new();
        return;
    }
    *w = trimmed;
}

pub fn read_masterdata_file(rel_path: &str) -> Option<Vec<u8>> {
    let base = MASTERDATA_PATH.read().unwrap().clone();
    if base.is_empty() {
        return None;
    }
    let full_path = format!("{}/{}", base, rel_path);
    match fs::read(&full_path) {
        Ok(bytes) => Some(bytes),
        Err(_) => {
            let mut warned = MASTERDATA_WARNED.lock().unwrap();
            if warned.insert(rel_path.to_string()) {
                println!("Couldn't find masterdata {}", rel_path);
            }
            None
        }
    }
}

// Only currently editable by the android so
pub fn set_easter_mode(enabled: bool) {
    let mut w = EASTER.write().unwrap();
    *w = enabled;
}

pub fn get_easter_mode() -> bool {
    *EASTER.read().unwrap()
}

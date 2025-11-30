use lazy_static::lazy_static;
use std::sync::RwLock;
use std::fs;

lazy_static! {
    static ref RUNNING: RwLock<bool> = RwLock::new(false);
    static ref DATAPATH: RwLock<String> = RwLock::new(String::new());
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

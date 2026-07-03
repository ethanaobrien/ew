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
    static ref MOD_PATHS: RwLock<Vec<String>> = RwLock::new(Vec::new());
    static ref EASTER: RwLock<bool> = RwLock::new(false);
    static ref HOST_CONFIG: RwLock<HostConfig> = RwLock::new(HostConfig::default());
}

// This is used by an embedding app in lib mode
#[derive(Default, Clone)]
pub struct HostConfig {
    pub assets_url: String,
    pub npps4: String,
    pub max_time: u64,
    pub port: u16,
    pub jp_android_asset_hash: String,
    pub en_android_asset_hash: String,
    pub enable_custom_songs: bool,
}

// Lets an embedding app (or the tests) enable the opt-in custom songs feature
// without a command-line flag
pub fn set_enable_custom_songs(enabled: bool) {
    HOST_CONFIG.write().unwrap().enable_custom_songs = enabled;
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

pub fn update_mod_paths(paths: &[String]) {
    let cleaned: Vec<String> = paths.iter()
        .map(|p| p.trim_end_matches('/').to_string())
        .filter(|p| !p.is_empty())
        .filter_map(|p| {
            if Path::new(&p).is_dir() {
                Some(p)
            } else {
                println!("Couldn't find mod directory {} — skipping", p);
                None
            }
        })
        .collect();
    if !cleaned.is_empty() {
        println!("Loaded {} mod overlay{}", cleaned.len(),
                 if cleaned.len() == 1 { "" } else { "s" });
        for p in &cleaned {
            println!("  mod: {}", p);
        }
    }
    let mut w = MOD_PATHS.write().unwrap();
    *w = cleaned;
}

pub fn read_mod_files(rel_path: &str) -> Vec<(String, Vec<u8>)> {
    let paths = MOD_PATHS.read().unwrap().clone();
    let mut out = Vec::new();
    for p in paths {
        let full = format!("{}/{}", p, rel_path);
        if let Ok(bytes) = fs::read(&full) {
            out.push((p, bytes));
        }
    }
    out
}

pub fn apply_config_json(json: &str) {
    let parsed = match jzon::parse(json) {
        Ok(p) => p,
        Err(e) => {
            println!("Ignoring invalid host config json: {}", e);
            return;
        }
    };

    if let Some(v) = parsed["dataPath"].as_str() {
        if !v.is_empty() {
            update_data_path(v);
        }
    }
    set_easter_mode(parsed["easterMode"].as_bool().unwrap_or(false));

    let s = |key: &str| parsed[key].as_str().unwrap_or("").to_string();
    let mut cfg = HOST_CONFIG.write().unwrap();
    cfg.assets_url = s("assetsUrl");
    cfg.npps4 = s("npps4");
    cfg.max_time = parsed["maxTime"].as_u64().unwrap_or(0);
    cfg.port = parsed["port"].as_u64().unwrap_or(0) as u16;
    cfg.jp_android_asset_hash = s("jpAndroidAssetHash");
    cfg.en_android_asset_hash = s("enAndroidAssetHash");
}

pub fn overlay_args(args: &mut crate::options::Args) {
    let cfg = HOST_CONFIG.read().unwrap();
    macro_rules! overlay_str {
        ($field:ident) => {
            if !cfg.$field.is_empty() {
                args.$field = cfg.$field.clone();
            }
        };
    }
    overlay_str!(assets_url);
    overlay_str!(npps4);
    if cfg.max_time != 0 {
        args.max_time = cfg.max_time;
    }
    if cfg.port != 0 {
        args.port = cfg.port;
    }
    overlay_str!(jp_android_asset_hash);
    overlay_str!(en_android_asset_hash);
    // Overlay only ever enables the feature; a command-line --enable-custom-songs
    // is never overridden back to off
    if cfg.enable_custom_songs {
        args.enable_custom_songs = true;
    }
}

// idk why an ai put tests here but they are here now. Yay tests????
#[cfg(test)]
lazy_static! {
    static ref TEST_DATA_DIR: String = {
        let dir = std::env::temp_dir().join(format!("ew-tests-{}", std::process::id()));
        let _ = fs::remove_dir_all(&dir);
        fs::create_dir_all(&dir).unwrap();
        dir.to_str().unwrap().to_string()
    };
    static ref TEST_LOCK: Mutex<()> = Mutex::new(());
}

#[cfg(test)]
pub fn lock_test_data_path() -> std::sync::MutexGuard<'static, ()> {
    let guard = crate::lock_onto_mutex!(TEST_LOCK);
    update_data_path(&TEST_DATA_DIR);
    // The feature is off by default; tests exercise it, so turn it on while holding the lock
    set_enable_custom_songs(true);
    guard
}

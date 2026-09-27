use lazy_static::lazy_static;
use std::collections::HashSet;
use std::path::Path;
use std::sync::{Mutex, RwLock};
use std::fs;

lazy_static! {
    static ref RUNNING: RwLock<bool> = RwLock::new(false);
    static ref DATAPATH: RwLock<String> = RwLock::new(String::new());
    static ref OWNERS: RwLock<Vec<i64>> = RwLock::new(Vec::new());
    static ref MASTERDATA_PATH: RwLock<String> = RwLock::new(String::new());
    static ref MASTERDATA_WARNED: Mutex<HashSet<String>> = Mutex::new(HashSet::new());
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
    pub asset_version: String,
    pub en_asset_version: String,
    pub jp_ios_asset_hash: String,
    pub en_ios_asset_hash: String,
    pub windows_asset_hash: String,
    pub enable_custom_songs: bool,
    pub enable_custom_cards: bool,
    pub enable_custom_3dmv: bool,
    pub enable_arcade: bool,
    pub nerf_custom_cards: bool,
    pub owner: Vec<i64>,
    pub hidden: bool,
    pub force_updates: bool,
    pub disable_imports: bool,
    pub disable_exports: bool,
    pub linux_asset_hash: String,
    pub macos_asset_hash: String,
    pub global_android: String,
    pub japan_android: String,
    pub global_ios: String,
    pub japan_ios: String,
    pub arcade_machine_ttl: u64,
    pub arcade_session_ttl: u64,
    pub image_asset_path: String,
    pub masterdata: String,
}

// Lets an embedding app (or the tests) enable the opt-in custom songs feature
// without a command-line flag
pub fn set_enable_custom_songs(enabled: bool) {
    HOST_CONFIG.write().unwrap().enable_custom_songs = enabled;
}

pub fn set_enable_custom_cards(enabled: bool) {
    HOST_CONFIG.write().unwrap().enable_custom_cards = enabled;
}

pub fn set_enable_custom_3dmv(enabled: bool) {
    HOST_CONFIG.write().unwrap().enable_custom_3dmv = enabled;
}

pub fn set_enable_arcade(enabled: bool) {
    HOST_CONFIG.write().unwrap().enable_arcade = enabled;
}

// The custom-card nerf band. Meaningful only with enable_custom_cards; kept
// separate so an embedding app can flip them independently
pub fn set_nerf_custom_cards(enabled: bool) {
    HOST_CONFIG.write().unwrap().nerf_custom_cards = enabled;
}

pub fn get_nerf_custom_cards() -> bool {
    HOST_CONFIG.read().unwrap().nerf_custom_cards
}

// The --owner uids: the permission system's bootstrap grantors. Process-level
// state rather than db rows so they work on a fresh install and can't be
// revoked through the webui
pub fn update_owners(uids: &[i64]) {
    let mut w = OWNERS.write().unwrap();
    *w = uids.to_vec();
}

pub fn get_owners() -> Vec<i64> {
    OWNERS.read().unwrap().clone()
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
    cfg.asset_version = s("assetVersion");
    cfg.en_asset_version = s("enAssetVersion");
    cfg.jp_ios_asset_hash = s("jpIosAssetHash");
    cfg.en_ios_asset_hash = s("enIosAssetHash");
    cfg.windows_asset_hash = s("windowsAssetHash");
    cfg.linux_asset_hash = s("linuxAssetHash");
    cfg.macos_asset_hash = s("macosAssetHash");
    cfg.global_android = s("globalAndroid");
    cfg.japan_android = s("japanAndroid");
    cfg.global_ios = s("globalIos");
    cfg.japan_ios = s("japanIos");
    cfg.arcade_machine_ttl = parsed["arcadeMachineTtl"].as_u64().unwrap_or(90);
    cfg.arcade_session_ttl = parsed["arcadeSessionTtl"].as_u64().unwrap_or(30);
    cfg.image_asset_path = s("imageAssetPath");
    cfg.masterdata = s("masterdata");
    cfg.owner = parsed["ownerUids"].members().filter_map(|v| v.as_i64()).collect();
    cfg.hidden = parsed["hidden"].as_bool().unwrap_or(false);
    cfg.force_updates = parsed["forceUpdates"].as_bool().unwrap_or(false);
    cfg.disable_imports = parsed["disableImports"].as_bool().unwrap_or(false);
    cfg.disable_exports = parsed["disableExports"].as_bool().unwrap_or(false);
    cfg.enable_custom_songs = parsed["enableCustomSongs"].as_bool().unwrap_or(false);
    cfg.enable_custom_cards = parsed["enableCustomCards"].as_bool().unwrap_or(false);
    cfg.nerf_custom_cards = parsed["nerfCustomCards"].as_bool().unwrap_or(false);
    cfg.enable_custom_3dmv = parsed["enableCustom3dmv"].as_bool().unwrap_or(false);
    cfg.enable_arcade = parsed["enableArcade"].as_bool().unwrap_or(false);
    let owners = cfg.owner.clone();
    let masterdata = cfg.masterdata.clone();
    let nerf_custom_cards = cfg.nerf_custom_cards;
    drop(cfg);
    update_owners(&owners);
    update_masterdata_path(&masterdata);
    set_nerf_custom_cards(nerf_custom_cards);
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
    overlay_str!(asset_version);
    overlay_str!(en_asset_version);
    overlay_str!(jp_ios_asset_hash);
    overlay_str!(en_ios_asset_hash);
    overlay_str!(windows_asset_hash);
    overlay_str!(linux_asset_hash);
    overlay_str!(macos_asset_hash);
    overlay_str!(global_android);
    overlay_str!(japan_android);
    overlay_str!(global_ios);
    overlay_str!(japan_ios);
    overlay_str!(image_asset_path);
    overlay_str!(masterdata);
    if cfg.arcade_machine_ttl != 0 {
        args.arcade_machine_ttl = cfg.arcade_machine_ttl;
    }
    if cfg.arcade_session_ttl != 0 {
        args.arcade_session_ttl = cfg.arcade_session_ttl;
    }
    args.owner = cfg.owner.clone();
    args.hidden = cfg.hidden;
    args.force_updates = cfg.force_updates;
    args.disable_imports = cfg.disable_imports;
    args.disable_exports = cfg.disable_exports;
    // Overlay only ever enables the features; a command-line --enable-custom-*
    // flag is never overridden back to off
    if cfg.enable_custom_songs {
        args.enable_custom_songs = true;
    }
    if cfg.enable_custom_cards {
        args.enable_custom_cards = true;
    }
    if cfg.enable_custom_3dmv {
        args.enable_custom_3dmv = true;
    }
    if cfg.enable_arcade {
        args.enable_arcade = true;
    }
    if cfg.nerf_custom_cards {
        args.nerf_custom_cards = true;
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
    // The features are off by default; tests exercise them, so turn them on
    // while holding the lock
    set_enable_custom_songs(true);
    set_enable_custom_cards(true);
    set_enable_custom_3dmv(true);
    set_enable_arcade(true);
    guard
}

#[cfg(test)]
#[test]
fn android_host_config_updates_owner_and_feature_options() {
    use clap::Parser;
    let _guard = lock_test_data_path();
    apply_config_json(r#"{"ownerUids":[42,84],"hidden":true,"forceUpdates":true,"disableImports":true,"disableExports":true,"enableCustomSongs":true,"enableCustomCards":true,"nerfCustomCards":true,"enableCustom3dmv":true,"enableArcade":true,"linuxAssetHash":"linux-hash","macosAssetHash":"mac-hash","globalAndroid":"https://example.com/gl.apk","arcadeMachineTtl":45,"arcadeSessionTtl":15}"#);
    let mut args = crate::options::Args::parse_from(["ew"]);
    overlay_args(&mut args);
    assert_eq!(get_owners(), vec![42, 84]);
    assert_eq!(args.owner, vec![42, 84]);
    assert!(args.hidden && args.force_updates && args.disable_imports && args.disable_exports);
    assert!(args.enable_custom_songs && args.enable_custom_cards && args.nerf_custom_cards);
    assert!(get_nerf_custom_cards());
    assert!(args.enable_custom_3dmv && args.enable_arcade);
    assert_eq!(args.linux_asset_hash, "linux-hash");
    assert_eq!(args.macos_asset_hash, "mac-hash");
    assert_eq!(args.global_android, "https://example.com/gl.apk");
    assert_eq!(args.arcade_machine_ttl, 45);
    assert_eq!(args.arcade_session_ttl, 15);

    apply_config_json(r#"{"ownerUids":[],"enableCustomSongs":false}"#);
    let mut reset = crate::options::Args::parse_from(["ew"]);
    overlay_args(&mut reset);
    assert!(get_owners().is_empty());
    assert!(!reset.enable_custom_songs);
    assert!(!reset.hidden);
    assert!(!get_nerf_custom_cards());
}

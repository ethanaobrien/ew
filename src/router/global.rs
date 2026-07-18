use jzon::{array, object, JsonValue};
use actix_web::{
    http::header::{HeaderMap, HeaderValue},
    HttpRequest,
    HttpResponse
};
use std::time::{SystemTime, UNIX_EPOCH};
use base64::{engine::general_purpose, Engine as _};
use uuid::Uuid;

use crate::encryption;
use crate::router::{items, userdata};
use crate::database::gree;
use crate::runtime::get_easter_mode;

struct AssetVersion {
    region:   &'static str,
    platform: &'static str,
    version:  &'static str,
    hash:     &'static str,
}

static ASSET_VERSIONS: &[AssetVersion] = &[
    // Default / stock
    AssetVersion { region: "JP", platform: "Android", version: "4c921d2443335e574a82e04ec9ea243c", hash: "67f8f261c16b3cca63e520a25aad6c1c" },
    AssetVersion { region: "JP", platform: "iOS",     version: "4c921d2443335e574a82e04ec9ea243c", hash: "b8975be8300013a168d061d3fdcd4a16" },
    AssetVersion { region: "GL", platform: "Android", version: "5260ff15dff8ba0c00ad91400f515f55", hash: "d210b28037885f3ef56b8f8aa45ac95b" },
    AssetVersion { region: "GL", platform: "iOS",     version: "5260ff15dff8ba0c00ad91400f515f55", hash: "dd7175e4bcdab476f38c33c7f34b5e4d" },

    // Re-written client versions 2.0.0 - 2.1.2
    AssetVersion { region: "JP", platform: "Windows", version: "4c921d2443335e574a82e04ec9ea243c", hash: "4ed1d077df2d1b29e17d25d64fb37242" },

    // Re-written client versions 2.2.0 - 
    AssetVersion { region: "JP", platform: "Windows", version: "ced44f266b4e4c8eb05fe417fd5f3d1b", hash: "13ff04b7d1e4a7353458b8607307bd6b" },
    
    //AssetVersion { region: "JP", platform: "WebGL",   version: "4c921d2443335e574a82e04ec9ea243c", hash: "e1ff7c74b20c8d216507972b6f24b9df" },
];

impl AssetVersion {
    fn version(&self, current: bool) -> String {
        let args = crate::get_args();

        let override_version = match (current, self.region, self.platform) {
            (true, "JP", "Windows") => args.windows_asset_version.as_str(),
            _                       => "",
        };

        if override_version.is_empty() {
            self.version
        } else {
            override_version
        }.to_string()
    }
    fn hash(&self, current: bool) -> String {
        let args = crate::get_args();

        let override_hash = match (current, self.region, self.platform) {
            (true, "JP", "Android") => args.jp_android_asset_hash.as_str(),
            (true, "JP", "iOS")     => args.jp_ios_asset_hash.as_str(),
            (true, "JP", "Windows") => args.windows_asset_hash.as_str(),
            (true, "GL", "Android") => args.en_android_asset_hash.as_str(),
            (true, "GL", "iOS")     => args.en_ios_asset_hash.as_str(),
            _                       => "",
        };
        if !override_hash.is_empty() {
            return override_hash.to_string();
        }

        if current && self.platform == "Android" && get_easter_mode() {
            if let Some((_, easter)) = EASTER_HASHES.iter().find(|(r, _)| *r == self.region) {
                return easter.to_string();
            }
        }

        self.hash.to_string()
    }
}

fn find_asset_hash(asset_version: &str, platform: &str) -> Option<String> {
    let mut seen_regions: Vec<&str> = Vec::new();
    for entry in ASSET_VERSIONS {
        if entry.platform != platform {
            continue;
        }
        let current = !seen_regions.contains(&entry.region);
        seen_regions.push(entry.region);
        if entry.version(current) == asset_version {
            return Some(entry.hash(current));
        }
    }
    None
}

static EASTER_HASHES: &[(&str, &str)] = &[
    ("JP", "eac0cad61c82bf2e31fc596555747d11"),
    ("GL", "da7ae831381c3f29337caa9891db7e6a"),
];

pub const RESULT_GAME_VERSION_UPDATED: i32 = 12;

pub const RESULT_RESOURCE_UPDATED: i32 = 13;

pub const PROTOCOL_HEADER: &str = "X-Protocol-Version";

pub fn client_protocol_version(req: &HttpRequest) -> u32 {
    req.headers()
        .get(PROTOCOL_HEADER)
        .and_then(|v| v.to_str().ok())
        .and_then(|v| v.trim().parse().ok())
        .unwrap_or(0)
}

pub fn get_player_region(asset_version: &str) -> Option<String> {
    ASSET_VERSIONS
        .iter()
        .find(|entry| entry.version == asset_version)
        .map(|entry| entry.region.to_string())
}

pub fn parse_platform(header: &str) -> &str {
    let platform = header.split_whitespace().next().unwrap_or("").to_lowercase();
    match platform.as_str() {
        "android" => "Android",
        "ios"     => "iOS",
        "iphoneplayer" => "iOS",
        "iphone" => "iOS",
        "windows" => "Windows",
        "windowsplayer" => "Windows",
        "webglplayer" => "WebGL",
        _         => panic!("Unknown platform: {header}"),
    }
}

pub fn get_asset_hash(asset_version: &str, platform: &str) -> Option<String> {
    let rv = find_asset_hash(asset_version, platform);
    println!("Get asset hash: {platform}. {rv:?}");
    rv
}

pub fn check_asset_headers(headers: &HeaderMap, check_hash: bool) -> Option<i32> {
    let blank_header = HeaderValue::from_static("");
    let asset_version = headers.get("aoharu-asset-version").unwrap_or(&blank_header).to_str().unwrap_or("");
    if asset_version.is_empty() {
        return None;
    }

    let platform = match headers.get("aoharu-platform").and_then(|v| v.to_str().ok()) {
        Some(header) => parse_platform(header),
        None => return None,
    };

    let current = match find_asset_hash(asset_version, platform) {
        Some(hash) => hash,
        None => return Some(RESULT_GAME_VERSION_UPDATED),
    };
    if !check_hash {
        return None;
    }

    let asset_hash = headers.get("aoharu-asset-hash").unwrap_or(&blank_header).to_str().unwrap_or("");
    if asset_hash.is_empty() || asset_hash == current {
        None
    } else {
        Some(RESULT_RESOURCE_UPDATED)
    }
}

pub fn create_token() -> String {
    format!("{}", Uuid::now_v7())
}

fn get_uuid(input: &str) -> Option<String> {
    let key = "sk1bdzb310n0s9tl";
    let key_index = match input.find(key) {
        Some(index) => index + key.len(),
        None => return None,
    };
    let after = &input[key_index..];

    let uuid_length = 36;
    if after.len() >= uuid_length {
        let uuid = &after[..uuid_length];
        return Some(uuid.to_string());
    }

    None
}
pub fn get_login(headers: &HeaderMap, body: &str) -> String {
    let blank_header = HeaderValue::from_static("");
    
    let login = headers.get("a6573cbe").unwrap_or(&blank_header).to_str().unwrap_or("");
    let decoded = general_purpose::STANDARD.decode(login).unwrap_or_default();
    match get_uuid(String::from_utf8_lossy(&decoded).as_ref()) {
        Some(token) => {
            token
        },
        None => {
            let rv = gree::get_uuid(headers, body);
            assert!(rv != String::new());
            rv
        },
    }
}

pub fn timestamp() -> u64 {
    let now = SystemTime::now();

    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();
    unix_timestamp.as_secs()
}

pub fn timestamp_msec() -> u32 {
    let now = SystemTime::now();

    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();
    unix_timestamp.subsec_nanos()
}

pub fn timestamp_since_midnight() -> u64 {
    let now = SystemTime::now();
    let unix_timestamp = now.duration_since(UNIX_EPOCH).unwrap();

    let midnight = unix_timestamp.as_secs() % (24 * 60 * 60);


    unix_timestamp.as_secs() - midnight
}

pub fn get_uid(headers: &HeaderMap) -> i64 {
    let blank_header = HeaderValue::from_static("");
    headers.get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("").parse::<i64>().unwrap_or(0)
}

fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}

pub fn format_datetime(time: u64) -> String {
    let days = (time / 86400) as i64;
    let secs = time % 86400;
    let (y, m, d) = civil_from_days(days);
    format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}", y, m, d, secs / 3600, (secs % 3600) / 60, secs % 60)
}

fn init_time(current_time: u64, server_data: &mut JsonValue, token: &str, max_time: u64, max: bool) {
    let mut edited = false;
    let default_time = 1709272800;

    if max_time > 10 && max_time < current_time && server_data["server_time"].as_u64().unwrap_or(0) < max_time && max {
        server_data["server_time_set"] = timestamp().into();
        edited = true;
    }

    if server_data["server_time_set"].as_u64().is_none() {
        server_data["server_time_set"] = timestamp().into();
        edited = true;
    }
    if server_data["server_time"].as_u64().is_none() {
        server_data["server_time"] = default_time.into();
        edited = true;
    }
    if edited && max {
        userdata::save_server_data(token, server_data.clone());
    }
}

pub fn set_time(current_time: u64, uid: i64, max: bool) -> u64 {
    let max_time = crate::get_args().max_time;
    if uid == 0 {
        if max_time > 10 && max_time < current_time {
            return max_time;
        } else {
            return timestamp();
        }
    }
    let token = userdata::get_login_token(uid);
    let mut server_data = userdata::get_server_data(&token);
    init_time(current_time, &mut server_data, &token, max_time, max);
    
    let time_set = server_data["server_time_set"].as_u64().unwrap_or(timestamp());
    let server_time = server_data["server_time"].as_u64().unwrap_or(0);//1711741114
    if server_time == 0 {
        return current_time;
    }
    
    let time_since_set = if current_time > time_set {
        current_time - time_set
    } else { 0 };
    return server_time + time_since_set;
}

pub fn send(mut data: JsonValue, uid: i64, headers: &HeaderMap) -> HttpResponse {
    //println!("{}", jzon::stringify(data.clone()));
    data["server_time"] = set_time(data["server_time"].as_u64().unwrap_or(0), uid, true).into();

    if !data["data"]["item_list"].is_empty() || !data["data"]["updated_value_list"]["item_list"].is_empty() {
        items::check_for_region(&mut data, headers);
    }
    
    let encrypted = encryption::encrypt_packet(&jzon::stringify(data)).unwrap();
    let resp = encrypted.into_bytes();

    HttpResponse::Ok().body(resp)
}

pub fn api(req: &HttpRequest, data: Option<JsonValue>) -> HttpResponse {
    let blank_header = HeaderValue::from_static("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("").parse::<i64>().unwrap_or(0);
    let rv = if let Some(data) = data {
        object!{
            "code": 0,
            "server_time": timestamp(),
            "data": data
        }
    } else {
        object!{
            "code": 4,
            "server_time": timestamp(),
            "message": ""
        }
    };
    send(rv, uid, req.headers())
}

pub fn api_error(req: &HttpRequest, code: i32) -> HttpResponse {
    let uid = get_uid(req.headers());
    send(object!{
        "code": code,
        "server_time": timestamp(),
        "message": ""
    }, uid, req.headers())
}

pub fn start_login_bonus(id: i64, bonus: &mut JsonValue) -> bool {
    if crate::router::login::get_login_bonus_info(id).is_empty() {
        return false;
    }
    for dataa in bonus["bonus_list"].members() {
        if dataa["master_login_bonus_id"].as_i64().unwrap() == id {
            return false;
        }
    }
    bonus["bonus_list"].push(object!{
        master_login_bonus_id: id,
        day_counts: [],
        event_bonus_list: []
    }).unwrap();
    true
}

pub fn get_card(id: i64, user: &JsonValue) -> JsonValue {
    if id == 0 {
        return object!{};
    }
    
    for data in user["card_list"].members() {
        if data["master_card_id"].as_i64().unwrap_or(0) == id {
            return data.clone();
        }
    }
    object!{}
}

pub(crate) fn get_cards(arr: JsonValue, user: &JsonValue) -> JsonValue {
    let mut rv = array![];
    for data in arr.members() {
        let to_push = get_card(data.as_i64().unwrap_or(0), user);
        if to_push.is_empty() {
            continue;
        }
        rv.push(to_push).unwrap();
    }
    rv
}

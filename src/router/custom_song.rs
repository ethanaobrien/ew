mod audio;
mod chart;
mod package;

use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, HttpResponse, Responder, http::header::ContentType};
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

use crate::router::{global, userdata, webui};
use crate::database::custom_song as database;
use crate::runtime::get_data_path;
use crate::lock_onto_mutex;

// Custom songs are owned by their uploader: only the owner can change or
// delete them through the webui. Visibility is per song - "public" (default,
// every user sees it), "private" (owner only) or "shared" (owner plus a list
// of user ids). Filtering happens at the CATALOG level (/api/custom_song/list
// and the user/get unlock list); the asset/audio GETs are content-addressed
// and sessionless, like a CDN.
//
// Storage layout (under --path):
//   custom_songs/{music_id}/jacket.png       512x512 png
//   custom_songs/{music_id}/jacket_blur.png  512x512 png, heavily blurred
//   custom_songs/{music_id}/chart_{level}.json
//   custom_songs/audio/{md5}.ogg             content-addressed vorbis oggs
// Metadata lives in custom_songs.db as one JSON blob per song, in the exact
// shape /api/custom_song/list serves.

// Shock.BAND_CATEGORY enum names
const BAND_CATEGORIES: &[&str] = &["NONE", "MUSE", "AQOURS", "NIJIGAKU", "LIELLA", "HASUNOSORA", "OTHER", "YOHANE"];

// NORMAL, HARD, EXPERT, MASTER
const LEVEL_COUNT: i64 = 4;
const DEFAULT_LEVEL_NUMBERS: &[i64] = &[3, 6, 9, 12];

const DEFAULT_BPM: f64 = 120.0;
const DEFAULT_PREVIEW_LENGTH_SEC: f64 = 30.0;
const PREVIEW_FADE_SEC: f64 = 0.5;

lazy_static! {
    // music_id assignment and the insert must not race between two uploads
    static ref UPLOAD_LOCK: Mutex<()> = Mutex::new(());
}

// Game endpoints (/api scope, standard envelope)
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/custom_song")
            .route("/list", web::post().to(list))
    );
}

// Plain asset GETs for the game + session-authenticated management API for the webui
pub fn web_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/custom_song")
            .route("/assets/{music_id}/{file}", web::get().to(assets))
            .route("/audio/{hash}/{file}", web::get().to(audio))
            .route("/upload", web::post().to(upload))
            .route("/update", web::post().to(update))
            .route("/mine", web::get().to(mine))
            .route("/browse", web::get().to(browse))
            .route("/download/{music_id}", web::get().to(download))
            .route("/visibility", web::post().to(visibility))
            .route("/delete", web::post().to(delete))
    );
}

// The whole feature is opt-in (--enable-custom-songs) and additionally off in
// --hidden mode. When disabled every endpoint 404s / errors as if it never
// existed, nothing touches custom_songs.db (so no table setup or migration
// runs), and no custom ids leak into unlock lists
pub fn disabled() -> bool {
    let args = crate::get_args();
    args.hidden || !args.enable_custom_songs
}

// A client that understands custom songs advertises it with this exact header
// on its API requests. Old / official clients don't send it, so we must NOT
// inject custom-song data (custom master_music_ids) into the shared /api/user
// response for them - the unresolvable ids would break the account
const SUPPORT_HEADER: &str = "X-Custom-Songs";

pub fn client_supports_custom_songs(req: &HttpRequest) -> bool {
    req.headers()
        .get(SUPPORT_HEADER)
        .and_then(|v| v.to_str().ok())
        == Some("1")
}

// The catalog is filtered per requesting user: private songs only show for
// their owner, shared songs for the owner plus their shared-user list
async fn list(req: HttpRequest, body: String) -> impl Responder {
    if disabled() {
        // As if the endpoint doesn't exist - the client treats this as feature-off
        return global::api(&req, None);
    }
    let key = global::get_login(req.headers(), &body);
    let uid = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    global::api(&req, Some(object!{
        "revision": database::get_revision(),
        "songs": database::get_songs_for_user(uid)
    }))
}

// Appended to the master_music_ids unlock list in user/get, filtered like the
// catalog. Empty (and touches no DB) when the feature is disabled
pub fn get_music_ids(uid: i64) -> JsonValue {
    if disabled() {
        return array![];
    }
    database::get_music_ids_for_user(uid)
}

fn song_path(music_id: i64, file: &str) -> String {
    get_data_path(&format!("custom_songs/{}/{}", music_id, file))
}

fn audio_file_path(md5: &str) -> String {
    get_data_path(&format!("custom_songs/audio/{}.ogg", md5))
}

async fn assets(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let music_id = req.match_info().get("music_id").unwrap_or("").parse::<i64>().unwrap_or(0);
    let file = req.match_info().get("file").unwrap_or("").to_string();
    let valid = file == "jacket.png" || file == "jacket_blur.png"
        || (1..=LEVEL_COUNT).any(|level| file == format!("chart_{}.json", level));
    if music_id < database::FIRST_MUSIC_ID || !valid {
        return HttpResponse::NotFound().finish();
    }
    match fs::read(song_path(music_id, &file)) {
        Ok(body) => {
            let mime = mime_guess::from_path(&file).first_or_octet_stream();
            HttpResponse::Ok()
                .insert_header(ContentType(mime))
                .insert_header(("content-length", body.len()))
                .body(body)
        },
        Err(_) => HttpResponse::NotFound().finish()
    }
}

// Matches the client's '{server}/{hash}/{name}.ogg' sound downloader format
async fn audio(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let hash = req.match_info().get("hash").unwrap_or("").to_string();
    let file = req.match_info().get("file").unwrap_or("").to_string();
    if hash.len() != 32 || !hash.chars().all(|c| c.is_ascii_hexdigit()) || file != format!("{}.ogg", hash) {
        return HttpResponse::NotFound().finish();
    }
    match fs::read(audio_file_path(&hash)) {
        Ok(body) => {
            HttpResponse::Ok()
                .insert_header(("content-type", "audio/ogg"))
                .insert_header(("content-length", body.len()))
                .body(body)
        },
        Err(_) => HttpResponse::NotFound().finish()
    }
}

fn get_session_uid(req: &HttpRequest) -> Option<i64> {
    let token = webui::get_login_token(req)?;
    let login_token = userdata::webui_login_token(&token)?;
    userdata::get_acc(&login_token)["user"]["id"].as_i64()
}

fn send_json(resp: JsonValue) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

async fn read_multipart(mut payload: Multipart) -> Result<HashMap<String, Vec<u8>>, String> {
    let mut fields = HashMap::new();
    while let Some(mut field) = payload.try_next().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();
        let mut data = Vec::new();
        while let Some(chunk) = field.try_next().await.map_err(|e| e.to_string())? {
            data.extend_from_slice(&chunk);
        }
        fields.insert(name, data);
    }
    Ok(fields)
}

fn field_str(fields: &HashMap<String, Vec<u8>>, key: &str) -> String {
    String::from_utf8_lossy(fields.get(key).map(|v| v.as_slice()).unwrap_or(&[])).trim().to_string()
}

fn field_f64(fields: &HashMap<String, Vec<u8>>, key: &str) -> Option<f64> {
    field_str(fields, key).parse::<f64>().ok()
}

// Checkbox-style flag: "1", "true" or "on"
fn field_flag(fields: &HashMap<String, Vec<u8>>, key: &str) -> bool {
    matches!(field_str(fields, key).to_lowercase().as_str(), "1" | "true" | "on")
}

// Shared users are designated by their numeric user id (the id shown on the
// account page and used for webui login/friend requests), comma-separated in
// the webui form
fn parse_shared_users(input: &str) -> Result<JsonValue, String> {
    let mut rv = array![];
    for part in input.split(',') {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let id = part.parse::<i64>().map_err(|_| format!("'{}' is not a valid user id", part))?;
        if !rv.contains(id) {
            rv.push(id).unwrap();
        }
    }
    Ok(rv)
}

fn validate_shared_users(shared_with: &JsonValue) -> Result<(), String> {
    for id in shared_with.members() {
        let Some(id) = id.as_i64() else {
            return Err(format!("'{}' is not a valid user id", id));
        };
        if userdata::get_login_token(id) == String::new() {
            return Err(format!("User {} does not exist", id));
        }
    }
    Ok(())
}

// Pad/crop the upload to a square, then resize to 512x512
fn process_jacket(bytes: &[u8]) -> Result<(Vec<u8>, Vec<u8>), String> {
    let img = image::load_from_memory(bytes).map_err(|_| String::from("Jacket is not a valid png/jpg image"))?;
    let size = std::cmp::min(img.width(), img.height());
    let jacket = img
        .crop_imm((img.width() - size) / 2, (img.height() - size) / 2, size, size)
        .resize_exact(512, 512, image::imageops::FilterType::Lanczos3);
    // Mirror the official _blur art: a heavily blurred copy of the jacket
    let blur = jacket.blur(24.0);

    let mut jacket_png = Vec::new();
    jacket.write_to(&mut std::io::Cursor::new(&mut jacket_png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    let mut blur_png = Vec::new();
    blur.write_to(&mut std::io::Cursor::new(&mut blur_png), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok((jacket_png, blur_png))
}

fn cue_json(cue: &audio::Cue, cue_name: String) -> JsonValue {
    object!{
        "cue_name": cue_name,
        "md5": cue.md5.clone(),
        "size": cue.bytes.len(),
        "duration_sec": cue.duration_sec as f32,
        "is_loop": true,
        "loop_start_sec": 0.0,
        "loop_end_sec": cue.duration_sec as f32
    }
}

// Score thresholds when the uploader doesn't provide any: take the highest
// difficulty's full_combo and stars, budget base = full_combo * 200 * (1 + stars / 10)
// (~200 points per note, scaled up for harder charts), then C/B/A/S at
// 50%/75%/100%/130% of base. Multi live thresholds are 1.2x the solo ones.
fn default_scores(full_combo: i64, level_number: i64) -> (JsonValue, JsonValue) {
    let base = full_combo as f64 * 200.0 * (1.0 + level_number as f64 / 10.0);
    let score = |mult: f64| (base * mult) as u32;
    (object!{
        "c": score(0.5), "b": score(0.75), "a": score(1.0), "s": score(1.3)
    }, object!{
        "c": score(0.5 * 1.2), "b": score(0.75 * 1.2), "a": score(1.0 * 1.2), "s": score(1.3 * 1.2)
    })
}

fn create_song(uid: i64, fields: &HashMap<String, Vec<u8>>) -> Result<i64, String> {
    let name = field_str(fields, "name");
    let artist = field_str(fields, "artist");
    if name.is_empty() || artist.is_empty() {
        return Err(String::from("Song name and artist are required"));
    }

    let attribute = field_str(fields, "attribute").parse::<i64>().unwrap_or(0);
    if !(1..=3).contains(&attribute) {
        return Err(String::from("Attribute must be 1 (smile), 2 (pure) or 3 (cool)"));
    }

    let mut band_category = field_str(fields, "band_category");
    if band_category.is_empty() {
        band_category = String::from("OTHER");
    }
    if !BAND_CATEGORIES.contains(&band_category.as_str()) {
        return Err(format!("Unknown band category '{}'", band_category));
    }

    let mut visibility = field_str(fields, "visibility");
    if visibility.is_empty() {
        visibility = String::from("public");
    }
    if !database::VISIBILITIES.contains(&visibility.as_str()) {
        return Err(format!("Unknown visibility '{}'", visibility));
    }
    let shared_with = parse_shared_users(&field_str(fields, "shared_with"))?;
    validate_shared_users(&shared_with)?;
    let downloads_disabled = field_flag(fields, "downloads_disabled");

    // (level, chart json, full_combo, level_number, original SIF1 bytes)
    let mut charts: Vec<(i64, JsonValue, i64, i64, Vec<u8>)> = Vec::new();
    for level in 1..=LEVEL_COUNT {
        let Some(raw) = fields.get(&format!("chart_{}", level)) else { continue; };
        if raw.is_empty() {
            continue;
        }
        let beatmap = jzon::parse(&String::from_utf8_lossy(raw))
            .map_err(|_| format!("Difficulty {}: chart is not valid JSON", level))?;
        let (chart, full_combo) = chart::transcode(&beatmap)
            .map_err(|e| format!("Difficulty {}: {}", level, e))?;
        let level_number = field_str(fields, &format!("level_number_{}", level))
            .parse::<i64>().unwrap_or(DEFAULT_LEVEL_NUMBERS[(level - 1) as usize]);
        charts.push((level, chart, full_combo, level_number, raw.clone()));
    }
    if charts.is_empty() {
        return Err(String::from("At least one difficulty chart is required"));
    }

    let jacket_bytes = fields.get("jacket").filter(|v| !v.is_empty())
        .ok_or(String::from("A jacket image is required"))?;
    let (jacket, jacket_blur) = process_jacket(jacket_bytes)?;

    let audio_bytes = fields.get("audio").filter(|v| !v.is_empty())
        .ok_or(String::from("An audio track is required"))?;
    let (play, select) = audio::process(audio_bytes, field_f64(fields, "preview_start_sec"), field_f64(fields, "preview_length_sec"))?;

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let music_id = database::next_music_id();

    let suffix = format!("Custom{}", music_id);
    let mut levels = array![];
    for (level, _, full_combo, level_number, _) in charts.iter() {
        levels.push(object!{
            "level": *level,
            "level_number": *level_number,
            "full_combo": *full_combo,
            "score_coeff": 1.0,
            // Official convention: the filename difficulty index is level+1
            "note_data_file_name": format!("{}_{}_{}", music_id, level + 1, suffix),
            "chart": format!("/custom_song/assets/{}/chart_{}.json", music_id, level)
        }).unwrap();
    }

    let (_, _, hardest_combo, hardest_stars, _) = charts.last().unwrap();
    let (score, multi_score) = default_scores(*hardest_combo, *hardest_stars);

    // The upload metadata in the multipart-field schema, kept alongside the
    // original artifacts so the song can be exported and re-uploaded elsewhere
    let mut manifest_levels = array![];
    for (level, _, _, level_number, _) in charts.iter() {
        manifest_levels.push(object!{
            "level": *level,
            "level_number": *level_number
        }).unwrap();
    }
    let manifest = object!{
        "format": 1,
        "name": name.clone(),
        "name_en": field_str(fields, "name_en"),
        "short_name": field_str(fields, "short_name"),
        "kana": field_str(fields, "kana"),
        "artist": artist.clone(),
        "artist_en": field_str(fields, "artist_en"),
        "attribute": attribute,
        "band_category": band_category.clone(),
        "bpm": field_f64(fields, "bpm"),
        "preview_start_sec": field_f64(fields, "preview_start_sec"),
        "preview_length_sec": field_f64(fields, "preview_length_sec"),
        "levels": manifest_levels
    };

    let song = object!{
        "music_id": music_id,
        "name": name,
        "name_en": field_str(fields, "name_en"),
        "short_name": field_str(fields, "short_name"),
        "kana": field_str(fields, "kana"),
        "artist": artist,
        "artist_en": field_str(fields, "artist_en"),
        "band_category": band_category.clone(),
        "master_group_id": database::band_group_id(&band_category),
        "attribute": attribute,
        "bpm": field_f64(fields, "bpm").unwrap_or(DEFAULT_BPM) as f32,
        "start_wait": 2.0,
        "end_wait": 0.0,
        "score": score,
        "multi_score": multi_score,
        // Combo missions at 25/50/75/100% of the hardest difficulty's full combo
        "mission_combo": [hardest_combo / 4, hardest_combo / 2, hardest_combo * 3 / 4, *hardest_combo],
        "jacket": format!("/custom_song/assets/{}/jacket.png", music_id),
        "levels": levels,
        "sound": {
            "cue_sheet": format!("song_{}_{}", music_id, suffix),
            "play": cue_json(&play, format!("play_{}_{}", music_id, suffix)),
            "select": cue_json(&select, format!("select_{}_{}", music_id, suffix))
        }
    };

    fs::create_dir_all(get_data_path(&format!("custom_songs/{}", music_id))).map_err(|e| e.to_string())?;
    fs::create_dir_all(get_data_path("custom_songs/audio")).map_err(|e| e.to_string())?;
    fs::write(song_path(music_id, "jacket.png"), jacket).map_err(|e| e.to_string())?;
    fs::write(song_path(music_id, "jacket_blur.png"), jacket_blur).map_err(|e| e.to_string())?;
    for (level, chart, _, _, _) in charts.iter() {
        fs::write(song_path(music_id, &format!("chart_{}.json", level)), jzon::stringify(chart.clone())).map_err(|e| e.to_string())?;
    }
    fs::write(audio_file_path(&play.md5), &play.bytes).map_err(|e| e.to_string())?;
    fs::write(audio_file_path(&select.md5), &select.bytes).map_err(|e| e.to_string())?;

    // The original upload artifacts. SIF1 is the canonical interchange format:
    // these exact bytes (plus the manifest) form the export package, and
    // importing one on another server replays this same upload pipeline
    fs::create_dir_all(get_data_path(&format!("custom_songs/{}/original", music_id))).map_err(|e| e.to_string())?;
    fs::write(song_path(music_id, "original/manifest.json"), jzon::stringify(manifest)).map_err(|e| e.to_string())?;
    fs::write(song_path(music_id, "original/jacket"), jacket_bytes).map_err(|e| e.to_string())?;
    fs::write(song_path(music_id, "original/audio"), audio_bytes).map_err(|e| e.to_string())?;
    for (level, _, _, _, raw) in charts.iter() {
        fs::write(song_path(music_id, &format!("original/chart_{}.json", level)), raw).map_err(|e| e.to_string())?;
    }

    database::insert_song(music_id, uid, &song, &visibility, &shared_with, downloads_disabled);
    database::bump_revision();
    drop(lock);

    Ok(music_id)
}

// Edit an existing song in place. The music_id - and everything derived from
// it: live_id, cue names, note_data_file_name, asset URLs - stays the same, so
// player score records survive (delete + re-upload retires the id and wipes
// them). A field present in the form replaces the stored value, an absent one
// keeps it; visibility/shared_with/downloads_disabled are not touched here.
// The stored originals under original/ are updated too, so a later export
// reflects the edited state.
fn update_song(music_id: i64, fields: &HashMap<String, Vec<u8>>) -> Result<(), String> {
    let old_song = database::get_song(music_id).ok_or(String::from("Song not found"))?;
    // Partial edits re-read the original upload artifacts (the manifest for
    // absent metadata fields, the original audio for preview re-cuts), which
    // songs from before export support don't have on disk
    let old_manifest = fs::read(song_path(music_id, "original/manifest.json"))
        .map_err(|_| String::from("This song was uploaded before export support and can't be edited"))?;
    let old_manifest = jzon::parse(&String::from_utf8_lossy(&old_manifest))
        .map_err(|_| String::from("This song was uploaded before export support and can't be edited"))?;

    // The stored values come from the manifest: it carries the upload-schema
    // fields, including the null-when-defaulted bpm/preview numbers
    let text = |key: &str| {
        if fields.contains_key(key) { field_str(fields, key) } else { old_manifest[key].as_str().unwrap_or("").to_string() }
    };
    let number = |key: &str| {
        if fields.contains_key(key) { field_f64(fields, key) } else { old_manifest[key].as_f64() }
    };

    let name = text("name");
    let artist = text("artist");
    if name.is_empty() || artist.is_empty() {
        return Err(String::from("Song name and artist are required"));
    }

    let attribute = if fields.contains_key("attribute") {
        field_str(fields, "attribute").parse::<i64>().unwrap_or(0)
    } else {
        old_manifest["attribute"].as_i64().unwrap_or(0)
    };
    if !(1..=3).contains(&attribute) {
        return Err(String::from("Attribute must be 1 (smile), 2 (pure) or 3 (cool)"));
    }

    let mut band_category = text("band_category");
    if band_category.is_empty() {
        band_category = String::from("OTHER");
    }
    if !BAND_CATEGORIES.contains(&band_category.as_str()) {
        return Err(format!("Unknown band category '{}'", band_category));
    }

    // (level, replacement chart json + original SIF1 bytes, full_combo, level_number)
    let mut charts: Vec<(i64, Option<(JsonValue, Vec<u8>)>, i64, i64)> = Vec::new();
    let mut removed: Vec<i64> = Vec::new();
    for level in 1..=LEVEL_COUNT {
        let existing = old_song["levels"].members().find(|data| data["level"] == level);
        let raw = fields.get(&format!("chart_{}", level)).filter(|v| !v.is_empty());
        if field_flag(fields, &format!("remove_chart_{}", level)) {
            if raw.is_some() {
                return Err(format!("Difficulty {}: cannot both replace and remove", level));
            }
            // Removing a difficulty the song doesn't have is a no-op
            if existing.is_some() {
                removed.push(level);
            }
            continue;
        }
        let stored_number = existing
            .and_then(|data| data["level_number"].as_i64())
            .unwrap_or(DEFAULT_LEVEL_NUMBERS[(level - 1) as usize]);
        let level_number = if fields.contains_key(&format!("level_number_{}", level)) {
            field_str(fields, &format!("level_number_{}", level)).parse::<i64>().unwrap_or(stored_number)
        } else {
            stored_number
        };
        if let Some(raw) = raw {
            // A chart for a level the song didn't have before ADDS that difficulty
            let beatmap = jzon::parse(&String::from_utf8_lossy(raw))
                .map_err(|_| format!("Difficulty {}: chart is not valid JSON", level))?;
            let (chart, full_combo) = chart::transcode(&beatmap)
                .map_err(|e| format!("Difficulty {}: {}", level, e))?;
            charts.push((level, Some((chart, raw.clone())), full_combo, level_number));
        } else if let Some(existing) = existing {
            charts.push((level, None, existing["full_combo"].as_i64().unwrap_or(0), level_number));
        }
    }
    if charts.is_empty() {
        return Err(String::from("At least one difficulty chart is required"));
    }

    let jacket_bytes = fields.get("jacket").filter(|v| !v.is_empty());
    let jacket = match jacket_bytes {
        Some(bytes) => Some(process_jacket(bytes)?),
        None => None
    };

    let preview_start_sec = number("preview_start_sec");
    let preview_length_sec = number("preview_length_sec");
    let audio_bytes = fields.get("audio").filter(|v| !v.is_empty());
    // New audio replaces both cues. A preview change without new audio re-cuts
    // the select cue from the stored original audio; the play cue stays put
    let (play, select) = if let Some(bytes) = audio_bytes {
        let (play, select) = audio::process(bytes, preview_start_sec, preview_length_sec)?;
        (Some(play), Some(select))
    } else if fields.contains_key("preview_start_sec") || fields.contains_key("preview_length_sec") {
        let original = fs::read(song_path(music_id, "original/audio")).map_err(|e| e.to_string())?;
        let (_, select) = audio::process(&original, preview_start_sec, preview_length_sec)?;
        (None, Some(select))
    } else {
        (None, None)
    };

    let suffix = format!("Custom{}", music_id);
    let mut levels = array![];
    for (level, _, full_combo, level_number) in charts.iter() {
        levels.push(object!{
            "level": *level,
            "level_number": *level_number,
            "full_combo": *full_combo,
            "score_coeff": 1.0,
            // Official convention: the filename difficulty index is level+1
            "note_data_file_name": format!("{}_{}_{}", music_id, level + 1, suffix),
            "chart": format!("/custom_song/assets/{}/chart_{}.json", music_id, level)
        }).unwrap();
    }

    // Scores and combo missions always derive from the resulting state, with
    // the same formulas as upload
    let (_, _, hardest_combo, hardest_stars) = charts.last().unwrap();
    let (score, multi_score) = default_scores(*hardest_combo, *hardest_stars);

    let mut manifest_levels = array![];
    for (level, _, _, level_number) in charts.iter() {
        manifest_levels.push(object!{
            "level": *level,
            "level_number": *level_number
        }).unwrap();
    }
    let manifest = object!{
        "format": 1,
        "name": name.clone(),
        "name_en": text("name_en"),
        "short_name": text("short_name"),
        "kana": text("kana"),
        "artist": artist.clone(),
        "artist_en": text("artist_en"),
        "attribute": attribute,
        "band_category": band_category.clone(),
        "bpm": number("bpm"),
        "preview_start_sec": preview_start_sec,
        "preview_length_sec": preview_length_sec,
        "levels": manifest_levels
    };

    // Same id everywhere, so the cue sheet/cue names never change
    let mut sound = old_song["sound"].clone();
    if let Some(play) = &play {
        sound["play"] = cue_json(play, format!("play_{}_{}", music_id, suffix));
    }
    if let Some(select) = &select {
        sound["select"] = cue_json(select, format!("select_{}_{}", music_id, suffix));
    }

    let song = object!{
        "music_id": music_id,
        "name": name,
        "name_en": text("name_en"),
        "short_name": text("short_name"),
        "kana": text("kana"),
        "artist": artist,
        "artist_en": text("artist_en"),
        "band_category": band_category.clone(),
        "master_group_id": database::band_group_id(&band_category),
        "attribute": attribute,
        "bpm": number("bpm").unwrap_or(DEFAULT_BPM) as f32,
        "start_wait": 2.0,
        "end_wait": 0.0,
        "score": score,
        "multi_score": multi_score,
        // Combo missions at 25/50/75/100% of the hardest difficulty's full combo
        "mission_combo": [hardest_combo / 4, hardest_combo / 2, hardest_combo * 3 / 4, *hardest_combo],
        "jacket": format!("/custom_song/assets/{}/jacket.png", music_id),
        "levels": levels,
        "sound": sound
    };

    // Same serialization as upload around the writes and the revision bump
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    if let (Some((jacket, jacket_blur)), Some(bytes)) = (&jacket, jacket_bytes) {
        fs::write(song_path(music_id, "jacket.png"), jacket).map_err(|e| e.to_string())?;
        fs::write(song_path(music_id, "jacket_blur.png"), jacket_blur).map_err(|e| e.to_string())?;
        fs::write(song_path(music_id, "original/jacket"), bytes).map_err(|e| e.to_string())?;
    }
    for (level, chart, _, _) in charts.iter() {
        if let Some((chart, raw)) = chart {
            fs::write(song_path(music_id, &format!("chart_{}.json", level)), jzon::stringify(chart.clone())).map_err(|e| e.to_string())?;
            fs::write(song_path(music_id, &format!("original/chart_{}.json", level)), raw).map_err(|e| e.to_string())?;
        }
    }
    for level in removed.iter() {
        let _ = fs::remove_file(song_path(music_id, &format!("chart_{}.json", level)));
        let _ = fs::remove_file(song_path(music_id, &format!("original/chart_{}.json", level)));
    }
    if let Some(play) = &play {
        fs::write(audio_file_path(&play.md5), &play.bytes).map_err(|e| e.to_string())?;
    }
    if let Some(select) = &select {
        fs::write(audio_file_path(&select.md5), &select.bytes).map_err(|e| e.to_string())?;
    }
    if let Some(bytes) = audio_bytes {
        fs::write(song_path(music_id, "original/audio"), bytes).map_err(|e| e.to_string())?;
    }
    fs::write(song_path(music_id, "original/manifest.json"), jzon::stringify(manifest)).map_err(|e| e.to_string())?;

    database::update_song(music_id, &song);
    database::bump_revision();
    drop(lock);

    // Replaced cues: the old oggs are content-addressed and may be shared with
    // another song (or unchanged by this edit) - GC them the same way delete does
    let kept = [song["sound"]["play"]["md5"].to_string(), song["sound"]["select"]["md5"].to_string()];
    for key in ["play", "select"] {
        let md5 = old_song["sound"][key]["md5"].to_string();
        if !md5.is_empty() && !kept.contains(&md5) && !database::audio_in_use(&md5, music_id) {
            let _ = fs::remove_file(audio_file_path(&md5));
        }
    }

    Ok(())
}

async fn upload(req: HttpRequest, payload: Multipart) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    let mut fields = match read_multipart(payload).await {
        Ok(fields) => fields,
        Err(e) => return webui::error(&e)
    };
    // An export package from another server: its contents map 1:1 onto the
    // normal upload fields, so importing is just an upload
    if let Some(bytes) = fields.remove("package") {
        if !bytes.is_empty() {
            if let Err(e) = package::expand(&bytes, &mut fields) {
                return webui::error(&e);
            }
        }
    }
    println!("UPLOAD5");
    match create_song(uid, &fields) {
        Ok(music_id) => send_json(object!{
            result: "OK",
            music_id: music_id
        }),
        Err(e) => webui::error(&e)
    }
}

// Owner-only: edit a song in place so charters don't have to delete and
// re-upload, which would assign a new music_id and wipe player score records
async fn update(req: HttpRequest, payload: Multipart) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    let fields = match read_multipart(payload).await {
        Ok(fields) => fields,
        Err(e) => return webui::error(&e)
    };
    let music_id = field_str(&fields, "music_id").parse::<i64>().unwrap_or(0);
    let Some(owner) = database::get_song_owner(music_id) else {
        return webui::error("Song not found");
    };
    if owner != uid {
        return webui::error("You can only manage your own songs");
    }
    match update_song(music_id, &fields) {
        Ok(()) => send_json(object!{
            result: "OK",
            music_id: music_id
        }),
        Err(e) => webui::error(&e)
    }
}

async fn mine(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    send_json(object!{
        result: "OK",
        songs: database::get_songs_by_owner(uid)
    })
}

// The public song browser. Anonymous viewers see the public catalog; a webui
// session additionally shows the viewer's own and shared-with-them songs
// (the same visibility rules as the game catalog)
async fn browse(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let viewer = get_session_uid(&req);
    let mut songs = database::get_browse_songs(viewer);
    for song in songs.members_mut() {
        song["uploader"] = userdata::get_name_and_rank(song["owner_id"].as_i64().unwrap())["user_name"].clone();
        song.remove("owner_id");
    }
    send_json(object!{
        result: "OK",
        songs: songs
    })
}

// Download a song as an export package, re-uploadable on any ew server. The
// viewer must be able to see the song, and downloads must be enabled unless
// they own it
async fn download(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let music_id = req.match_info().get("music_id").unwrap_or("").parse::<i64>().unwrap_or(0);
    if let Err(e) = database::export_allowed(music_id, get_session_uid(&req)) {
        return webui::error(e);
    }
    match package::build(music_id) {
        Ok(bytes) => {
            HttpResponse::Ok()
                .insert_header(("content-type", "application/zip"))
                .insert_header(("content-disposition", format!("attachment; filename=\"custom_song_{}.zip\"", music_id)))
                .insert_header(("content-length", bytes.len()))
                .body(bytes)
        },
        Err(e) => webui::error(&e)
    }
}

// Owner-only: change a song's visibility and/or its shared-user list
async fn visibility(req: HttpRequest, body: String) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    let body = jzon::parse(&body).unwrap_or(object!{});
    let music_id = body["music_id"].as_i64().unwrap_or(0);
    let Some(owner) = database::get_song_owner(music_id) else {
        return webui::error("Song not found");
    };
    if owner != uid {
        return webui::error("You can only manage your own songs");
    }
    let visibility = body["visibility"].to_string();
    if !database::VISIBILITIES.contains(&visibility.as_str()) {
        return webui::error(&format!("Unknown visibility '{}'", visibility));
    }
    let shared_with = body["shared_with"].clone();
    if let Err(e) = validate_shared_users(&shared_with) {
        return webui::error(&e);
    }

    database::set_visibility(music_id, &visibility, &shared_with);
    // The download toggle only affects the webui browser, not the game catalog
    if !body["downloads_disabled"].is_null() {
        database::set_downloads_disabled(music_id, body["downloads_disabled"].as_bool().unwrap_or(false));
    }
    database::bump_revision();

    send_json(object!{
        result: "OK"
    })
}

async fn delete(req: HttpRequest, body: String) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    let body = jzon::parse(&body).unwrap_or(object!{});
    let music_id = body["music_id"].as_i64().unwrap_or(0);
    let Some(owner) = database::get_song_owner(music_id) else {
        return webui::error("Song not found");
    };
    if owner != uid {
        return webui::error("You can only delete your own songs");
    }
    let song = database::get_song(music_id).unwrap_or(object!{});

    database::delete_song(music_id);
    database::bump_revision();
    // Global clear-rate stats for the dead live id (per-user score records are
    // wiped lazily on each user's next userdata pull)
    crate::router::clear_rate::purge_live(music_id);

    let _ = fs::remove_dir_all(get_data_path(&format!("custom_songs/{}", music_id)));
    // Audio is content-addressed and may be shared with another upload
    for key in ["play", "select"] {
        let md5 = song["sound"][key]["md5"].to_string();
        if !md5.is_empty() && !database::audio_in_use(&md5, music_id) {
            let _ = fs::remove_file(audio_file_path(&md5));
        }
    }

    send_json(object!{
        result: "OK"
    })
}




/// WHY DID THE AI WRITE 400 LINES OF TESTS
/// Well I guess they can't hurt lets commit them anyway

#[cfg(test)]
mod tests {
    use super::*;

    // 2 seconds of 44.1kHz 16-bit mono silence
    fn test_wav() -> Vec<u8> {
        let sample_rate: u32 = 44100;
        let data_len: u32 = sample_rate * 2 * 2;
        let mut rv = Vec::new();
        rv.extend(b"RIFF");
        rv.extend((36 + data_len).to_le_bytes());
        rv.extend(b"WAVEfmt ");
        rv.extend(16u32.to_le_bytes());
        rv.extend(1u16.to_le_bytes());
        rv.extend(1u16.to_le_bytes());
        rv.extend(sample_rate.to_le_bytes());
        rv.extend((sample_rate * 2).to_le_bytes());
        rv.extend(2u16.to_le_bytes());
        rv.extend(16u16.to_le_bytes());
        rv.extend(b"data");
        rv.extend(data_len.to_le_bytes());
        rv.resize(rv.len() + data_len as usize, 0);
        rv
    }

    // 2 seconds of a 440Hz sine, encoded to ogg-vorbis in-process
    fn test_ogg() -> Vec<u8> {
        test_ogg_tone(440.0)
    }

    // A distinct tone gives a song cues that aren't content-shared with the
    // other tests' songs (the oggs are content-addressed by md5)
    fn test_ogg_tone(freq: f32) -> Vec<u8> {
        let samples: Vec<f32> = (0..44100 * 2)
            .map(|i| (i as f32 * freq * 2.0 * std::f32::consts::PI / 44100.0).sin() * 0.5)
            .collect();
        let mut out = Vec::new();
        let mut builder = vorbis_rs::VorbisEncoderBuilder::new_with_serial(
            std::num::NonZeroU32::new(44100).unwrap(),
            std::num::NonZeroU8::new(1).unwrap(),
            &mut out,
            1
        );
        let mut encoder = builder.build().unwrap();
        encoder.encode_audio_block([&samples]).unwrap();
        encoder.finish().unwrap();
        out
    }

    fn test_png() -> Vec<u8> {
        let mut rv = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 8) as u8, 128, 255])
        })).write_to(&mut std::io::Cursor::new(&mut rv), image::ImageFormat::Png).unwrap();
        rv
    }

    fn test_chart() -> Vec<u8> {
        jzon::stringify(jzon::array![
            {"timing_sec": 0.5, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 5},
            {"timing_sec": 1.0, "notes_attribute": 1, "notes_level": 1, "effect": 3, "effect_value": 0.5, "position": 3},
            {"timing_sec": 1.5, "notes_attribute": 1, "notes_level": 1, "effect": 4, "effect_value": 0.0, "position": 7}
        ]).into_bytes()
    }

    fn field(fields: &mut HashMap<String, Vec<u8>>, key: &str, value: &str) {
        fields.insert(String::from(key), value.as_bytes().to_vec());
    }

    // Export a song, import the package as another user, and the served song
    // must be identical apart from the assigned music_id - INCLUDING the audio
    // md5s: ogg uploads are stored as-is and the preview encode is
    // deterministic, so both cues carry identical bytes on both servers
    #[test]
    fn export_import_round_trip() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Round Trip");
        field(&mut fields, "name_en", "Round Trip EN");
        field(&mut fields, "kana", "ラウンドトリップ");
        field(&mut fields, "artist", "Trip Artist");
        field(&mut fields, "attribute", "2");
        field(&mut fields, "band_category", "MUSE");
        field(&mut fields, "bpm", "182.5");
        field(&mut fields, "preview_start_sec", "0.5");
        field(&mut fields, "preview_length_sec", "1.0");
        field(&mut fields, "level_number_1", "7");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg());
        fields.insert(String::from("chart_1"), test_chart());
        let source_id = create_song(1111, &fields).unwrap();

        let zip = package::build(source_id).unwrap();
        let mut fields = HashMap::new();
        field(&mut fields, "visibility", "private");
        package::expand(&zip, &mut fields).unwrap();
        let imported_id = create_song(2222, &fields).unwrap();

        assert_ne!(source_id, imported_id);
        assert_eq!(database::get_song_owner(imported_id), Some(2222));

        let source = database::get_song(source_id).unwrap();
        let imported = database::get_song(imported_id).unwrap();
        for key in ["name", "name_en", "short_name", "kana", "artist", "artist_en", "band_category", "attribute", "bpm", "start_wait", "end_wait", "score", "multi_score", "mission_combo"] {
            assert_eq!(jzon::stringify(source[key].clone()), jzon::stringify(imported[key].clone()), "{}", key);
        }
        for (a, b) in source["levels"].members().zip(imported["levels"].members()) {
            for key in ["level", "level_number", "full_combo", "score_coeff"] {
                assert_eq!(a[key], b[key], "{}", key);
            }
        }
        assert_eq!(imported["levels"][0]["note_data_file_name"].to_string(), format!("{}_2_Custom{}", imported_id, imported_id));
        for key in ["md5", "size", "duration_sec", "is_loop", "loop_start_sec", "loop_end_sec"] {
            assert_eq!(source["sound"]["play"][key], imported["sound"]["play"][key], "play {}", key);
            assert_eq!(source["sound"]["select"][key], imported["sound"]["select"][key], "select {}", key);
        }
        // The stored play ogg is the upload itself, byte for byte
        assert_eq!(fs::read(audio_file_path(&source["sound"]["play"]["md5"].to_string())).unwrap(), test_ogg());
        // The preview is the requested cut: 1 second starting at 0.5
        assert!((source["sound"]["select"]["duration_sec"].as_f64().unwrap() - 1.0).abs() < 0.05);

        // Charts and jackets are deterministic: byte-identical on both servers
        for file in ["chart_1.json", "jacket.png", "jacket_blur.png", "original/chart_1.json", "original/jacket", "original/audio", "original/manifest.json"] {
            assert_eq!(fs::read(song_path(source_id, file)).unwrap(), fs::read(song_path(imported_id, file)).unwrap(), "{}", file);
        }
        let md5 = imported["sound"]["play"]["md5"].to_string();
        assert_eq!(fs::read(audio_file_path(&md5)).unwrap().len(), imported["sound"]["play"]["size"].as_usize().unwrap());
    }

    // Updates edit a song in place: the music_id stays, absent fields keep
    // their stored values, scores re-derive from the resulting state, and the
    // stored originals follow the edit so exports stay accurate
    #[test]
    fn update_edits_in_place() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Original Name");
        field(&mut fields, "artist", "Original Artist");
        field(&mut fields, "attribute", "1");
        field(&mut fields, "level_number_1", "5");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg_tone(660.0));
        fields.insert(String::from("chart_1"), test_chart());
        let music_id = create_song(1212, &fields).unwrap();
        let before = database::get_song(music_id).unwrap();
        let revision = database::get_revision();

        // Metadata-only edit: present fields replace, absent fields stay, the
        // cues are untouched and the revision bumps exactly once
        let mut fields = HashMap::new();
        field(&mut fields, "music_id", &music_id.to_string());
        field(&mut fields, "name", "New Name");
        field(&mut fields, "attribute", "3");
        update_song(music_id, &fields).unwrap();
        let song = database::get_song(music_id).unwrap();
        assert_eq!(song["music_id"], music_id);
        assert_eq!(song["name"], "New Name");
        assert_eq!(song["artist"], "Original Artist");
        assert_eq!(song["attribute"], 3);
        assert_eq!(song["sound"]["play"]["md5"], before["sound"]["play"]["md5"]);
        assert_eq!(song["sound"]["select"]["md5"], before["sound"]["select"]["md5"]);
        assert_eq!(database::get_revision(), revision + 1);

        // Adding a difficulty re-derives scores from the new hardest chart,
        // and the manifest follows so exports reflect the edited state
        let mut fields = HashMap::new();
        fields.insert(String::from("chart_4"), test_chart());
        field(&mut fields, "level_number_4", "12");
        update_song(music_id, &fields).unwrap();
        let song = database::get_song(music_id).unwrap();
        assert_eq!(song["levels"].len(), 2);
        let (score, _) = default_scores(3, 12);
        assert_eq!(song["score"]["s"], score["s"]);
        assert!(fs::read(song_path(music_id, "chart_4.json")).is_ok());
        assert!(fs::read(song_path(music_id, "original/chart_4.json")).is_ok());
        let manifest = jzon::parse(&String::from_utf8_lossy(&fs::read(song_path(music_id, "original/manifest.json")).unwrap())).unwrap();
        assert_eq!(manifest["name"], "New Name");
        assert_eq!(manifest["levels"].len(), 2);

        // Difficulty removal: replace+remove conflicts error, removing every
        // difficulty errors, removing one of two works and deletes its files
        let mut fields = HashMap::new();
        field(&mut fields, "remove_chart_4", "1");
        fields.insert(String::from("chart_4"), test_chart());
        assert_eq!(update_song(music_id, &fields).unwrap_err(), "Difficulty 4: cannot both replace and remove");
        let mut fields = HashMap::new();
        field(&mut fields, "remove_chart_1", "1");
        field(&mut fields, "remove_chart_4", "1");
        assert_eq!(update_song(music_id, &fields).unwrap_err(), "At least one difficulty chart is required");
        let mut fields = HashMap::new();
        field(&mut fields, "remove_chart_4", "1");
        update_song(music_id, &fields).unwrap();
        let song = database::get_song(music_id).unwrap();
        assert_eq!(song["levels"].len(), 1);
        assert!(fs::read(song_path(music_id, "chart_4.json")).is_err());
        assert!(fs::read(song_path(music_id, "original/chart_4.json")).is_err());

        // A preview edit alone re-cuts the select cue from the stored original
        // audio, keeps the play cue and garbage-collects the old select ogg
        let mut fields = HashMap::new();
        field(&mut fields, "preview_start_sec", "0.25");
        field(&mut fields, "preview_length_sec", "1.0");
        update_song(music_id, &fields).unwrap();
        let song = database::get_song(music_id).unwrap();
        assert_eq!(song["sound"]["play"]["md5"], before["sound"]["play"]["md5"]);
        assert_ne!(song["sound"]["select"]["md5"], before["sound"]["select"]["md5"]);
        assert!((song["sound"]["select"]["duration_sec"].as_f64().unwrap() - 1.0).abs() < 0.05);
        assert!(fs::read(audio_file_path(&song["sound"]["select"]["md5"].to_string())).is_ok());
        assert!(fs::read(audio_file_path(&before["sound"]["select"]["md5"].to_string())).is_err());
    }

    // mp3/wav uploads still work: symphonia decodes them and the play cue is
    // transcoded to ogg-vorbis in-process
    #[test]
    fn wav_uploads_are_transcoded() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Wav Song");
        field(&mut fields, "artist", "Wav Artist");
        field(&mut fields, "attribute", "1");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_wav());
        fields.insert(String::from("chart_1"), test_chart());
        let music_id = create_song(8888, &fields).unwrap();

        let song = database::get_song(music_id).unwrap();
        assert!((song["sound"]["play"]["duration_sec"].as_f64().unwrap() - 2.0).abs() < 0.05);
        // The stored cue really is ogg-vorbis now
        let ogg = fs::read(audio_file_path(&song["sound"]["play"]["md5"].to_string())).unwrap();
        assert!(ogg.starts_with(b"OggS"));
    }

    #[test]
    fn corrupt_audio_is_rejected() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut base = HashMap::new();
        field(&mut base, "name", "Bad Audio");
        field(&mut base, "artist", "Bad Artist");
        field(&mut base, "attribute", "1");
        base.insert(String::from("jacket"), test_png());
        base.insert(String::from("chart_1"), test_chart());

        // Garbage, garbage wearing an ogg header, and a truncated ogg
        for bad in [vec![7u8; 4096], [b"OggS".to_vec(), vec![7u8; 4096]].concat(), test_ogg()[..200].to_vec()] {
            let mut fields = base.clone();
            fields.insert(String::from("audio"), bad);
            let error = create_song(8888, &fields).unwrap_err();
            assert!(error.contains("Could not read audio file") || error.contains("corrupt or truncated"), "{}", error);
        }
        // Too-short audio still has its own error
        let samples: Vec<f32> = vec![0.0; 4410];
        let mut out = Vec::new();
        let mut builder = vorbis_rs::VorbisEncoderBuilder::new_with_serial(
            std::num::NonZeroU32::new(44100).unwrap(),
            std::num::NonZeroU8::new(1).unwrap(),
            &mut out,
            1
        );
        let mut encoder = builder.build().unwrap();
        encoder.encode_audio_block([&samples]).unwrap();
        encoder.finish().unwrap();
        let mut fields = base.clone();
        fields.insert(String::from("audio"), out);
        assert_eq!(create_song(8888, &fields).unwrap_err(), "Audio track is too short");
    }

    #[test]
    fn browse_respects_visibility() {
        let _lock = crate::runtime::lock_test_data_path();
        let owner = 3333;
        let friend = 4444;
        let stranger = 5555;

        let public_id = database::next_music_id();
        database::insert_song(public_id, owner, &object!{music_id: public_id}, "public", &array![], false);
        let private_id = database::next_music_id();
        database::insert_song(private_id, owner, &object!{music_id: private_id}, "private", &array![], false);
        let shared_id = database::next_music_id();
        database::insert_song(shared_id, owner, &object!{music_id: shared_id}, "shared", &array![friend], false);

        let has = |songs: &JsonValue, id: i64| songs.members().any(|data| data["music_id"] == id);

        let anonymous = database::get_browse_songs(None);
        assert!(has(&anonymous, public_id) && !has(&anonymous, private_id) && !has(&anonymous, shared_id));
        let for_owner = database::get_browse_songs(Some(owner));
        assert!(has(&for_owner, public_id) && has(&for_owner, private_id) && has(&for_owner, shared_id));
        assert!(for_owner.members().find(|data| data["music_id"] == public_id).unwrap()["mine"].as_bool().unwrap());
        let for_friend = database::get_browse_songs(Some(friend));
        assert!(has(&for_friend, public_id) && !has(&for_friend, private_id) && has(&for_friend, shared_id));
        let for_stranger = database::get_browse_songs(Some(stranger));
        assert!(has(&for_stranger, public_id) && !has(&for_stranger, private_id) && !has(&for_stranger, shared_id));
    }

    #[test]
    fn download_rules_are_enforced() {
        let _lock = crate::runtime::lock_test_data_path();
        let owner = 6666;
        let stranger = 7777;

        let locked_id = database::next_music_id();
        database::insert_song(locked_id, owner, &object!{music_id: locked_id}, "public", &array![], true);
        let open_id = database::next_music_id();
        database::insert_song(open_id, owner, &object!{music_id: open_id}, "public", &array![], false);
        let private_id = database::next_music_id();
        database::insert_song(private_id, owner, &object!{music_id: private_id}, "private", &array![], false);

        // Downloads disabled: everyone but the owner is denied
        assert!(database::export_allowed(locked_id, Some(owner)).is_ok());
        assert_eq!(database::export_allowed(locked_id, Some(stranger)), Err("The uploader has disabled downloads for this song"));
        assert_eq!(database::export_allowed(locked_id, None), Err("The uploader has disabled downloads for this song"));
        // Open public song: anyone, even anonymous
        assert!(database::export_allowed(open_id, None).is_ok());
        // Invisible songs don't admit they exist
        assert_eq!(database::export_allowed(private_id, Some(stranger)), Err("Song not found"));
        assert_eq!(database::export_allowed(9999999, Some(owner)), Err("Song not found"));
        // The toggle is reversible
        database::set_downloads_disabled(locked_id, false);
        assert!(database::export_allowed(locked_id, Some(stranger)).is_ok());

        // A row without stored originals (uploaded before export support) has
        // a clear error instead of a broken zip
        assert!(package::build(open_id).unwrap_err().contains("before export support"));
    }

    // master_group_id must never be 0 (the client's group filter crashes on it)
    // and must be the band's misc GroupMst id
    #[test]
    fn master_group_id_maps_per_band() {
        let _lock = crate::runtime::lock_test_data_path();

        let expected = [
            ("MUSE", 199), ("AQOURS", 299), ("NIJIGAKU", 399),
            ("LIELLA", 499), ("HASUNOSORA", 599), ("YOHANE", 9999),
            ("OTHER", 9999), ("NONE", 9999)
        ];
        for (band, group) in expected {
            let mut fields = HashMap::new();
            field(&mut fields, "name", "Group Test");
            field(&mut fields, "artist", "Group Artist");
            field(&mut fields, "attribute", "1");
            field(&mut fields, "band_category", band);
            fields.insert(String::from("jacket"), test_png());
            fields.insert(String::from("audio"), test_ogg());
            fields.insert(String::from("chart_1"), test_chart());
            let music_id = create_song(1234, &fields).unwrap();

            let song = database::get_song(music_id).unwrap();
            assert_eq!(song["master_group_id"], group, "band {}", band);
            assert_ne!(song["master_group_id"], 0, "band {}", band);
        }

        // Nothing in the catalog ever serves a 0
        for song in database::get_songs_for_user(1234).members() {
            assert_ne!(song["master_group_id"], 0);
        }
    }

    // The whole feature is off unless --enable-custom-songs: endpoints 404 / go
    // empty, and the webui config the client gates its nav on reports it off.
    // When enabled everything works.
    #[test]
    fn feature_gate_hides_everything_when_disabled() {
        let _lock = crate::runtime::lock_test_data_path();

        // Disabled: representative endpoint behaves as if absent, no ids leak,
        // and serverInfo tells the webui to hide the nav (header.js gates on it)
        crate::runtime::set_enable_custom_songs(false);
        assert!(disabled());
        assert!(get_music_ids(1).is_empty());
        let resp = actix_web::rt::System::new().block_on(async {
            browse(actix_web::test::TestRequest::default().to_http_request()).await
        });
        assert_eq!(resp.status(), actix_web::http::StatusCode::NOT_FOUND);
        let info = webui_server_info();
        assert_eq!(info["data"]["custom_songs"], false);

        // Enabled: browse serves the catalog again and serverInfo advertises it
        crate::runtime::set_enable_custom_songs(true);
        assert!(!disabled());
        let resp = actix_web::rt::System::new().block_on(async {
            browse(actix_web::test::TestRequest::default().to_http_request()).await
        });
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        let info = webui_server_info();
        assert_eq!(info["data"]["custom_songs"], true);
    }

    // The JSON body the webui's /api/webui/serverInfo handler returns
    fn webui_server_info() -> JsonValue {
        let resp = crate::router::webui::server_info(actix_web::test::TestRequest::default().to_http_request());
        let body = actix_web::rt::System::new().block_on(async {
            actix_web::body::to_bytes(resp.into_body()).await.unwrap()
        });
        jzon::parse(&String::from_utf8_lossy(&body)).unwrap()
    }

    // With the feature enabled, custom unlock ids are appended to /api/user ONLY
    // for clients that send X-Custom-Songs: 1. An old/official client (no
    // header, or a different value) gets its official unlock list untouched.
    #[test]
    fn unlock_ids_gated_on_support_header() {
        use actix_web::test::TestRequest;
        let _lock = crate::runtime::lock_test_data_path();

        let owner = 4242;
        let music_id = database::next_music_id();
        database::insert_song(music_id, owner, &object!{music_id: music_id}, "public", &array![], false);
        // Sanity: the feature is on, so the id is visible to get_music_ids
        assert!(get_music_ids(owner).contains(music_id));

        // Exactly what the /api/user handler appends to master_music_ids
        let appended = |req: &HttpRequest| -> JsonValue {
            if client_supports_custom_songs(req) {
                get_music_ids(owner)
            } else {
                array![]
            }
        };

        // No header -> unsupported -> nothing appended
        let without = TestRequest::default().to_http_request();
        assert!(!client_supports_custom_songs(&without));
        assert!(appended(&without).is_empty());

        // Correct header -> supported -> custom id appended
        let with = TestRequest::default().insert_header(("X-Custom-Songs", "1")).to_http_request();
        assert!(client_supports_custom_songs(&with));
        assert!(appended(&with).contains(music_id));

        // Any other value is treated as an unsupporting client
        let wrong = TestRequest::default().insert_header(("X-Custom-Songs", "true")).to_http_request();
        assert!(!client_supports_custom_songs(&wrong));
        assert!(appended(&wrong).is_empty());
    }
}

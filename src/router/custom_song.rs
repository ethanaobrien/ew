// audio is shared: custom_card voicelines transcode through the same
// in-process symphonia + vorbis machinery
pub mod audio;
mod chart;
// One-time startup regroup of charts stored before the spawn-group pairing rule; called from
// run_server, no-op when the feature is disabled or every chart is already correctly grouped
pub mod migrate;
mod package;

use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, HttpResponse, Responder, http::header::ContentType};
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::fs;
use std::sync::Mutex;

use crate::router::{global, rich_text, userdata, webui, Login, Api};
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

// Upload limits, enforced while the multipart field is still streaming (the 25MB
// PayloadConfig in lib.rs binds the String/Bytes extractors, not Multipart), and
// again over a package's expanded contents. The binding item is the audio track:
// 64MB holds a five-minute 44.1kHz stereo WAV or any realistic ogg/mp3, and the
// same figure is custom_3dmv's per-file cap. The per-request cap is twice that,
// which covers audio + jacket + four charts, or an export package plus the field
// map it expands into (the package field itself is removed before expansion)
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_REQUEST_BYTES: usize = 128 * 1024 * 1024;

// The longest track that may be decoded. Enforced inside the decoder's packet
// loop, not after it: the decode accumulates planar f32 PCM, so an hour-long
// input is ~1.4GB of Vec before anything downstream gets to reject it. Official
// lives run 2-3 minutes; ten is already far past any real chart
pub const MAX_AUDIO_SECONDS: f64 = 600.0;

// The largest jacket the image decoder is allowed to allocate for. Checked as a
// dimension/allocation limit BEFORE decode (image 0.25's own default is a 512MB
// allocation ceiling and no dimension bound at all), so a 40000x40000 png that
// compresses to a few KB is refused instead of decoded
const MAX_JACKET_DIM: u32 = 8192;
// The dimension cap is the binding one (8192^2 RGBA is 256MiB); this is the
// backstop for a decoder that wants scratch beyond the final buffer, and it sits
// below the crate's own 512MB default
const MAX_JACKET_ALLOC_BYTES: u64 = 384 * 1024 * 1024;

// Per-account storage quota, counted over the sizes the catalog quotes to the
// client (both jackets, every chart, both audio cues). The original upload
// artifacts kept under original/ roughly double the on-disk figure, so 2GiB of
// catalog bytes is ~4GiB of disk - about two hundred songs, an order of magnitude
// past any real uploader, while keeping one account from filling the volume
pub const MAX_BYTES_PER_USER: i64 = 2 * 1024 * 1024 * 1024;

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
            .route("/data/{hash}/{file}", web::get().to(data))
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

// Custom songs need protocol version 1 (global::PROTOCOL_HEADER). Clients
// below it don't understand the feature, so we must NOT inject custom-song
// data (custom master_music_ids) into the shared /api/user response for
// them - the unresolvable ids would break the account
pub fn client_supports_custom_songs(req: &HttpRequest) -> bool {
    global::client_protocol_version(req) >= 1
}

// The catalog is filtered per requesting user: private songs only show for
// their owner, shared songs for the owner plus their shared-user list
async fn list(Login(key): Login) -> impl Responder {
    if disabled() {
        // As if the endpoint doesn't exist - the client treats this as feature-off
        return Api(None);
    }
    let uid = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    let mut songs = database::get_songs_for_user(uid);
    for song in songs.members_mut() {
        // Additive field: the client turns it into the song's detail-info credit line (the
        // staff-credits text the live loading screen and the music library show). Old clients
        // that don't know the field simply ignore it. The name is an ACCOUNT name, which the
        // profile route stores verbatim, so it is stripped of rich-text tags before it lands in
        // a TMP rich-text field (rich_text.rs)
        let Some(music_id) = song["music_id"].as_i64() else { continue; };
        let owner = database::get_song_owner(music_id).unwrap_or(0);
        let name = userdata::get_name_and_rank(owner)["user_name"].as_str().unwrap_or("").to_string();
        song["uploader"] = rich_text::strip_tags(&name).into();
    }
    Api(Some(object!{
        "revision": database::get_revision(),
        "songs": songs
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

pub fn hidden_live_ids() -> JsonValue {
    if disabled() {
        return array![];
    }
    database::non_public_music_ids()
}

// The clear-rate page shows real titles for PUBLIC custom songs; anything
// else stays exactly as hidden as before
pub fn public_song_title(music_id: i64, english: bool) -> Option<String> {
    if disabled() {
        return None;
    }
    database::public_song_title(music_id, english)
}

pub fn hidden_live_ids_for_user(uid: i64) -> JsonValue {
    if disabled() {
        return array![];
    }
    database::non_public_music_ids_for(uid)
}

// Whether `uid` may attach cross-feature content (a custom 3D MV) to this
// song: it must exist, and be theirs or publicly visible. Mirrors
// custom_card::validate_character_ref
pub fn can_reference_song(uid: i64, music_id: i64) -> Result<(), String> {
    if !disabled()
        && database::get_song_owner(music_id).is_some()
        && (database::get_song_owner(music_id) == Some(uid) || database::song_publicly_visible(music_id)) {
        return Ok(());
    }
    Err(format!("Unknown music_id '{}'", music_id))
}

fn song_path(music_id: i64, file: &str) -> String {
    get_data_path(&format!("custom_songs/{}/{}", music_id, file))
}

fn audio_file_path(md5: &str) -> String {
    get_data_path(&format!("custom_songs/audio/{}.ogg", md5))
}

// Startup GC for the content-addressed audio store. Every ogg in there belongs
// to some song's play or select cue; the only things that write the directory
// are upload and update, so an unreferenced file is a leftover from an
// interrupted one. Deleting it loses nothing - the same audio uploaded again
// re-encodes to the same bytes at the same path (audio.rs).
//
// Deliberately conservative, because the failure mode is deleting audio a live
// song serves: anything that makes the reference set doubtful - an unreadable
// catalog, a blob that won't parse, a song with no cue md5 - aborts the whole
// sweep instead of treating that song as referencing nothing. Only exactly
// {32 hex}.ogg names are ever considered. Takes UPLOAD_LOCK for the same
// reason delete does: an upload that has written its oggs but not yet inserted
// its row must not look like an orphan.
pub fn sweep_audio() {
    if disabled() {
        return;
    }
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let Some(blobs) = database::all_song_blobs() else {
        println!("Custom song audio sweep: catalog unreadable, skipped");
        return;
    };
    let mut referenced: Vec<String> = Vec::new();
    for blob in blobs.members() {
        let Ok(song) = jzon::parse(&blob.to_string()) else {
            println!("Custom song audio sweep: unparseable catalog row, skipped");
            return;
        };
        for key in ["play", "select"] {
            let md5 = song["sound"][key]["md5"].as_str().unwrap_or("");
            if md5.is_empty() {
                println!("Custom song audio sweep: song {} has no {} cue, skipped", song["music_id"], key);
                return;
            }
            referenced.push(String::from(md5));
        }
    }

    // No directory means nothing was ever uploaded
    let Ok(entries) = fs::read_dir(get_data_path("custom_songs/audio")) else {
        return;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(md5) = name.strip_suffix(".ogg") else { continue; };
        if md5.len() != 32 || !md5.chars().all(|c| c.is_ascii_hexdigit()) {
            continue;
        }
        if referenced.iter().any(|other| other == md5) {
            continue;
        }
        if fs::remove_file(entry.path()).is_ok() {
            removed += 1;
        }
    }
    if removed > 0 {
        println!("Custom song audio sweep: removed {} orphaned ogg(s)", removed);
    }
    drop(lock);
}

// The per-id asset files, for the webui's song pages (the game client fetches
// jackets and charts through /data/{md5} instead - the URLs this route serves are
// only ever followed by a browser that carries the webui session cookie).
//
// Unlike /data and /audio this route is addressed by a SEQUENTIAL id, not by an
// unguessable content hash, so the "the hash is the capability" argument that
// makes those two sessionless does not apply: without a visibility check the
// whole private and shared catalog's jackets and full chart JSON could be walked
// by anyone from music_id 10000 up. It gets the catalog's own rule
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
    // A song the viewer may not see 404s rather than admitting it exists
    if !database::asset_visible(music_id, get_session_uid(&req)) {
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

// Content-addressed chart/jacket fetch, same '{server}/{hash}/{name}.{ext}'
// shape as the audio route. The game client builds the URL from the md5 it
// reads in the catalog (charts -> .json, jackets -> .png); the ext is cosmetic,
// the md5 resolves the bytes. Visible-to-all like the other asset routes (CDN
// semantics) - only the feature flag gates it. A changed asset has a new md5,
// so a stale md5 simply 404s and the client re-downloads under the new one.
async fn data(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let hash = req.match_info().get("hash").unwrap_or("").to_string();
    let file = req.match_info().get("file").unwrap_or("").to_string();
    if hash.len() != 32 || !hash.chars().all(|c| c.is_ascii_hexdigit()) || !file.starts_with(&format!("{}.", hash)) {
        return HttpResponse::NotFound().finish();
    }
    let Some((music_id, filename)) = database::find_asset_by_md5(&hash) else {
        return HttpResponse::NotFound().finish();
    };
    match fs::read(song_path(music_id, &filename)) {
        // Jackets and charts live at FIXED per-song filenames that an in-place edit
        // overwrites, so between the file write and the catalog update the index
        // still points an old md5 at bytes that are no longer its own. The client
        // caches whatever it downloads under the md5 it asked for and never
        // re-checks, so serving those bytes would poison its cache permanently.
        // The index is a hint; these bytes are the answer only if they hash to the
        // request. A mismatch is the same 404 a stale md5 already gets, and the
        // client re-downloads under the md5 the catalog now carries
        Ok(body) if !hash.eq_ignore_ascii_case(&format!("{:x}", md5::compute(&body))) => {
            HttpResponse::NotFound().finish()
        },
        Ok(body) => {
            let mime = mime_guess::from_path(&filename).first_or_octet_stream();
            HttpResponse::Ok()
                .insert_header(ContentType(mime))
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

// The per-file cap is enforced while the field is still streaming, BEFORE any byte
// reaches the audio decoder, the png decoder or the chart parser. The per-request
// cap is checked over the running total
async fn read_multipart(mut payload: Multipart) -> Result<HashMap<String, Vec<u8>>, String> {
    let mut fields = HashMap::new();
    let mut total = 0usize;
    while let Some(mut field) = payload.try_next().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();
        let mut data = Vec::new();
        while let Some(chunk) = field.try_next().await.map_err(|e| e.to_string())? {
            total += chunk.len();
            if total > MAX_REQUEST_BYTES {
                return Err(over_request_limit());
            }
            data.extend_from_slice(&chunk);
            if data.len() > MAX_FILE_BYTES {
                return Err(over_file_limit(&name));
            }
        }
        fields.insert(name, data);
    }
    Ok(fields)
}

pub fn over_file_limit(name: &str) -> String {
    format!("'{}' exceeds the {} MB per-file limit", name, MAX_FILE_BYTES / (1024 * 1024))
}

pub fn over_request_limit() -> String {
    format!("Upload exceeds the {} MB per-request limit", MAX_REQUEST_BYTES / (1024 * 1024))
}

// The same accounting read_multipart applies, re-run over a field map that came
// out of an export package. package::expand caps every entry as it inflates, but
// the caps have to hold over the RESULT too: a package is one multipart field and
// its expansion replaces the whole form
fn check_field_caps(fields: &HashMap<String, Vec<u8>>) -> Result<(), String> {
    let mut total = 0usize;
    for (name, data) in fields.iter() {
        if data.len() > MAX_FILE_BYTES {
            return Err(over_file_limit(name));
        }
        total += data.len();
        if total > MAX_REQUEST_BYTES {
            return Err(over_request_limit());
        }
    }
    Ok(())
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
    // Dimension and allocation limits BEFORE the decode: the header is read, the
    // pixels are not, so a highly compressed enormous image is refused instead of
    // being expanded into memory
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(MAX_JACKET_DIM);
    limits.max_image_height = Some(MAX_JACKET_DIM);
    limits.max_alloc = Some(MAX_JACKET_ALLOC_BYTES);
    let mut reader = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .map_err(|_| String::from("Jacket is not a valid png/jpg image"))?;
    reader.limits(limits);
    let img = reader.decode().map_err(|e| match e {
        image::ImageError::Limits(_) => format!("Jacket is larger than the {}x{} limit", MAX_JACKET_DIM, MAX_JACKET_DIM),
        _ => String::from("Jacket is not a valid png/jpg image")
    })?;
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

// is_loop follows the official cue convention, and it is load-bearing: the client's CriWare
// layer reports a LOOP cue as forever-playing, so Playback.IsPlayEnd() never turns true for it.
// The live's end trigger (LiveTimeController.UpdateFree: isMusicEnded -> InLiveDelay -> EndWait)
// hangs off exactly that signal, so a looping PLAY cue means the live never ends. Only the
// music-select PREVIEW cue loops, like the official select bgm.
fn cue_json(cue: &audio::Cue, cue_name: String, is_loop: bool) -> JsonValue {
    object!{
        "cue_name": cue_name,
        "md5": cue.md5.clone(),
        "size": cue.bytes.len(),
        "duration_sec": cue.duration_sec as f32,
        "is_loop": is_loop,
        "loop_start_sec": 0.0,
        "loop_end_sec": if is_loop { cue.duration_sec as f32 } else { 0.0 }
    }
}

// A cue's stored md5, or None when the blob has no usable one. JsonValue::to_string
// renders Null as the literal "null", so a missing md5 used to arrive at the GC as
// a five-character string that passed an is_empty() guard; only exactly 32 hex
// characters is a hash (the same test the startup sweep and custom_3dmv's GC use)
fn cue_md5(cue: &JsonValue) -> Option<String> {
    let md5 = cue["md5"].as_str()?;
    if md5.len() != 32 || !md5.chars().all(|c| c.is_ascii_hexdigit()) {
        return None;
    }
    Some(md5.to_string())
}

// (md5-hex, byte-length) of a downloadable asset's served bytes. The client
// caches charts/jackets content-addressed by this md5, so it must be the hash
// of the exact bytes the data route returns
fn asset_meta(bytes: &[u8]) -> (String, usize) {
    (format!("{:x}", md5::compute(bytes)), bytes.len())
}

// Score rank thresholds. These are NOT per-song in SIF2: all 637 rows of the official live
// masterdata carry the exact same C/B/A/S tuple, solo and multi alike (live.csv columns
// _scoreC.._multiScoreS), because the score depends on deck strength rather than chart size.
// Deriving them from the chart's note count instead made rank S trivial on a short custom
// chart and unreachable on a long one, and skewed everything else that reads them - the live
// score gauge and the deck-confirm score estimation both scale off _scoreS/_multiScoreS
// (LiveData.MaxScore = _scoreS * 5 / 4).
const OFFICIAL_SCORE: [i64; 4] = [20000, 100000, 250000, 350000];
const OFFICIAL_MULTI_SCORE: [i64; 4] = [70000, 350000, 875000, 1225000];

fn default_scores() -> (JsonValue, JsonValue) {
    (object!{
        "c": OFFICIAL_SCORE[0], "b": OFFICIAL_SCORE[1], "a": OFFICIAL_SCORE[2], "s": OFFICIAL_SCORE[3]
    }, object!{
        "c": OFFICIAL_MULTI_SCORE[0], "b": OFFICIAL_MULTI_SCORE[1], "a": OFFICIAL_MULTI_SCORE[2], "s": OFFICIAL_MULTI_SCORE[3]
    })
}

// Every free-text column the client shows for a song. It renders them through TMP with rich
// text on and no escaping (rich_text.rs), and the official music table carries no markup in ANY
// of these columns - <br> only ever appears in detailInfo - so no tag is allowed in any of them.
fn validate_song_text(
    name: &str, name_en: &str, short_name: &str, kana: &str, artist: &str, artist_en: &str
) -> Result<(), String> {
    for (label, text) in [
        ("Song name", name),
        ("Song English name", name_en),
        ("Short name", short_name),
        ("Name reading", kana),
        ("Artist", artist),
        ("English artist", artist_en)
    ] {
        rich_text::reject_tags(label, text, &[])?;
    }
    Ok(())
}

// Combo-mission targets. Official live_mission_combo rows are round(hardest difficulty's
// full combo * 0.2/0.4/0.6/0.8) - verified against 626 of the 637 shipped rows (the 11
// outliers are songs that gained a harder difficulty after the mission row was authored).
// The previous 25/50/75/100% spread made the fourth mission demand a literal FULL COMBO of
// the hardest chart, a target no official song ever sets.
fn mission_combo(hardest_combo: i64) -> JsonValue {
    let target = |fraction: f64| (hardest_combo as f64 * fraction + 0.5) as i64;
    jzon::array![target(0.2), target(0.4), target(0.6), target(0.8)]
}

// The live's count-in, and the longest a marker can be in flight before its note. The count-in
// is LiveMst._startWait, 2.0 in every official live row and in ours. The flight time is
// LiveUtils.GetMarkerMoveTime(speed) = 1.725 - 0.125 * speed, clamped at 0.1, where speed is the
// player's per-difficulty rhythm-icon setting; its slowest end (and even a hypothetical 0) stays
// under the count-in, so a note at t >= 0 always has room to travel. Kept explicit so the check
// below stays honest if either constant ever moves.
const START_WAIT_SEC: f64 = 2.0;
const MAX_MARKER_MOVE_SEC: f64 = 1.725;

// A chart has to fit INSIDE its audio, at both ends.
//
// Tail: the live ends the moment the audio does - LiveTimeController's m_MusicDuration is
// LiveMst._endWait + the music length and _endWait is 0 - so a note whose MISS window closes
// after that is never judged. The player cannot full-combo the chart (the combo missions and
// the FULL COMBO banner both compare against full_combo, which counts every note), and the
// trailing markers are still on screen when the result screen takes over.
//
// Head: a marker spawns at time - GetMarkerMoveTime and the chart clock starts at -_startWait,
// so a note earlier than the flight time minus the count-in would pop in already halfway down
// the lane.
fn validate_chart_fits_audio(level: i64, chart: &JsonValue, duration_sec: f64) -> Result<(), String> {
    let end = chart::end_time(chart);
    if end > duration_sec {
        return Err(format!(
            "Difficulty {}: the chart needs {:.2}s but the audio is only {:.2}s long - the last note is never judged, because the live ends when the music does",
            level, end, duration_sec
        ));
    }
    if let Some(first) = chart::first_note_time(chart) {
        let earliest = MAX_MARKER_MOVE_SEC - START_WAIT_SEC;
        if first < earliest {
            return Err(format!(
                "Difficulty {}: the first note is at {:.2}s, before the {:.2}s the live needs to bring a marker down the lane",
                level, first, earliest
            ));
        }
    }
    Ok(())
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

    validate_song_text(
        &name,
        &field_str(fields, "name_en"),
        &field_str(fields, "short_name"),
        &field_str(fields, "kana"),
        &artist,
        &field_str(fields, "artist_en")
    )?;

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

    for (level, chart, _, _, _) in charts.iter() {
        validate_chart_fits_audio(*level, chart, play.duration_sec)?;
    }

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let music_id = database::next_music_id();

    let suffix = format!("Custom{}", music_id);
    let mut levels = array![];
    for (level, chart, full_combo, level_number, _) in charts.iter() {
        // md5/size over the exact chart JSON bytes written to disk and served
        let (md5, size) = asset_meta(jzon::stringify(chart.clone()).as_bytes());
        levels.push(object!{
            "level": *level,
            "level_number": *level_number,
            "full_combo": *full_combo,
            "score_coeff": 1.0,
            // Official convention: the filename difficulty index is level+1
            "note_data_file_name": format!("{}_{}_{}", music_id, level + 1, suffix),
            "chart": format!("/custom_song/assets/{}/chart_{}.json", music_id, level),
            "md5": md5,
            "size": size
        }).unwrap();
    }
    let (jacket_md5, jacket_size) = asset_meta(&jacket);
    let (jacket_blur_md5, jacket_blur_size) = asset_meta(&jacket_blur);

    let (_, _, hardest_combo, _, _) = charts.last().unwrap();
    let (score, multi_score) = default_scores();

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
        // Combo missions at the official 20/40/60/80% of the hardest difficulty's full combo
        "mission_combo": mission_combo(*hardest_combo),
        "jacket": format!("/custom_song/assets/{}/jacket.png", music_id),
        "jacket_md5": jacket_md5,
        "jacket_size": jacket_size,
        "jacket_blur": format!("/custom_song/assets/{}/jacket_blur.png", music_id),
        "jacket_blur_md5": jacket_blur_md5,
        "jacket_blur_size": jacket_blur_size,
        "levels": levels,
        "sound": {
            "cue_sheet": format!("song_{}_{}", music_id, suffix),
            "play": cue_json(&play, format!("play_{}_{}", music_id, suffix), false),
            "select": cue_json(&select, format!("select_{}_{}", music_id, suffix), true)
        }
    };

    check_quota(uid, database::song_bytes(&song), 0)?;

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

    database::insert_song(music_id, uid, &song, &visibility, &shared_with, downloads_disabled)
        .map_err(|e| format!("Could not store the song: {}", e))?;
    database::bump_revision();
    drop(lock);

    Ok(music_id)
}

// Per-account storage quota. `excluded` is the song being replaced by an in-place
// edit - its stored size drops out and `adding` (the resulting size) replaces it
fn check_quota(uid: i64, adding: i64, excluded_music_id: i64) -> Result<(), String> {
    let used = database::owner_bytes(uid, excluded_music_id);
    if used + adding > MAX_BYTES_PER_USER {
        return Err(format!(
            "This upload would put your songs at {} MB, over the {} MB per-account limit - delete a song first",
            (used + adding) / (1024 * 1024), MAX_BYTES_PER_USER / (1024 * 1024)
        ));
    }
    Ok(())
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

    // The RESULTING text, so an edit that leaves a field alone is checked against what stays
    validate_song_text(
        &name, &text("name_en"), &text("short_name"), &text("kana"), &artist, &text("artist_en")
    )?;

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

    // Every chart in the RESULTING song has to fit the RESULTING audio, so replacing either
    // side re-checks the other: new audio is validated against the charts that stay, and a new
    // chart against the audio that stays (read back from the catalog's own cue metadata).
    let play_duration = match &play {
        Some(play) => Some(play.duration_sec),
        None => old_song["sound"]["play"]["duration_sec"].as_f64()
    };
    if let Some(duration) = play_duration {
        for (level, chart, _, _) in charts.iter() {
            match chart {
                Some((chart, _)) => validate_chart_fits_audio(*level, chart, duration)?,
                None => {
                    let path = song_path(music_id, &format!("chart_{}.json", level));
                    if let Ok(bytes) = fs::read(&path) {
                        if let Ok(stored) = jzon::parse(&String::from_utf8_lossy(&bytes)) {
                            validate_chart_fits_audio(*level, &stored, duration)?;
                        }
                    }
                }
            }
        }
    }

    let suffix = format!("Custom{}", music_id);
    let mut levels = array![];
    for (level, chart, full_combo, level_number) in charts.iter() {
        // md5/size track the on-disk bytes: a replaced chart hashes its new
        // bytes, an unchanged one re-hashes the existing file (so even a song
        // edited before md5 fields existed gets a complete, correct catalog)
        let (md5, size) = match chart {
            Some((chart, _)) => asset_meta(jzon::stringify(chart.clone()).as_bytes()),
            None => asset_meta(&fs::read(song_path(music_id, &format!("chart_{}.json", level))).map_err(|e| e.to_string())?)
        };
        levels.push(object!{
            "level": *level,
            "level_number": *level_number,
            "full_combo": *full_combo,
            "score_coeff": 1.0,
            // Official convention: the filename difficulty index is level+1
            "note_data_file_name": format!("{}_{}_{}", music_id, level + 1, suffix),
            "chart": format!("/custom_song/assets/{}/chart_{}.json", music_id, level),
            "md5": md5,
            "size": size
        }).unwrap();
    }
    let (jacket_md5, jacket_size, jacket_blur_md5, jacket_blur_size) = match &jacket {
        Some((jacket, blur)) => {
            let (jm, js) = asset_meta(jacket);
            let (bm, bs) = asset_meta(blur);
            (jm, js, bm, bs)
        },
        None => {
            let (jm, js) = asset_meta(&fs::read(song_path(music_id, "jacket.png")).map_err(|e| e.to_string())?);
            let (bm, bs) = asset_meta(&fs::read(song_path(music_id, "jacket_blur.png")).map_err(|e| e.to_string())?);
            (jm, js, bm, bs)
        }
    };

    // Scores and combo missions always derive from the resulting state, with
    // the same formulas as upload
    let (_, _, hardest_combo, _) = charts.last().unwrap();
    let (score, multi_score) = default_scores();

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
        sound["play"] = cue_json(play, format!("play_{}_{}", music_id, suffix), false);
    }
    if let Some(select) = &select {
        sound["select"] = cue_json(select, format!("select_{}_{}", music_id, suffix), true);
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
        // Combo missions at the official 20/40/60/80% of the hardest difficulty's full combo
        "mission_combo": mission_combo(*hardest_combo),
        "jacket": format!("/custom_song/assets/{}/jacket.png", music_id),
        "jacket_md5": jacket_md5,
        "jacket_size": jacket_size,
        "jacket_blur": format!("/custom_song/assets/{}/jacket_blur.png", music_id),
        "jacket_blur_md5": jacket_blur_md5,
        "jacket_blur_size": jacket_blur_size,
        "levels": levels,
        "sound": sound
    };

    // The resulting song has to fit the uploader's quota, with the stored copy of
    // this same song excluded (it is being replaced, not added to)
    let owner = database::get_song_owner(music_id).ok_or(String::from("Song not found"))?;
    check_quota(owner, database::song_bytes(&song), music_id)?;

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

    // Replaced cues: the old oggs are content-addressed and may be shared with
    // another song (or unchanged by this edit) - GC them the same way delete does.
    // INSIDE the lock, for the reason delete states: an upload writes its oggs
    // before it inserts its row, so a GC that reads the catalog in that window
    // sees no reference to a shared md5 and unlinks the file the new song is
    // about to serve. A read error means "assume referenced" - never unlink
    let kept: Vec<String> = ["play", "select"].iter().filter_map(|key| cue_md5(&song["sound"][*key])).collect();
    for key in ["play", "select"] {
        let Some(md5) = cue_md5(&old_song["sound"][key]) else { continue; };
        if !kept.contains(&md5) && !database::audio_in_use(&md5, music_id).unwrap_or(true) {
            let _ = fs::remove_file(audio_file_path(&md5));
        }
    }
    drop(lock);

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
    // Everything from here on is seconds of CPU - zip inflation, a png decode and
    // resize, a whole-track vorbis encode - so it runs on the blocking pool
    // instead of the actix worker that has to keep serving the game API
    let result = web::block(move || {
        // An export package from another server: its contents map 1:1 onto the
        // normal upload fields, so importing is just an upload
        if let Some(bytes) = fields.remove("package") {
            if !bytes.is_empty() {
                package::expand(&bytes, &mut fields)?;
                // The expansion replaced the form: it has to satisfy the same
                // per-file/per-request caps the multipart reader enforces
                check_field_caps(&fields)?;
            }
        }
        create_song(uid, &fields)
    }).await;
    match result {
        Ok(Ok(music_id)) => send_json(object!{
            result: "OK",
            music_id: music_id
        }),
        Ok(Err(e)) => webui::error(&e),
        Err(_) => webui::error("The upload could not be processed")
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
    // Same blocking-pool treatment as upload: an edit can re-encode the audio and
    // re-derive the jackets
    match web::block(move || update_song(music_id, &fields)).await {
        Ok(Ok(())) => send_json(object!{
            result: "OK",
            music_id: music_id
        }),
        Ok(Err(e)) => webui::error(&e),
        Err(_) => webui::error("The edit could not be processed")
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

    if let Err(e) = database::set_visibility(music_id, &visibility, &shared_with) {
        return webui::error(&format!("Could not change the visibility: {}", e));
    }
    // The download toggle only affects the webui browser, not the game catalog
    if !body["downloads_disabled"].is_null() {
        database::set_downloads_disabled(music_id, body["downloads_disabled"].as_bool().unwrap_or(false));
    }
    database::bump_revision();
    crate::router::clear_rate::invalidate_cache();

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

    // Same lock upload and update take, for the audio GC below: an upload
    // writes its oggs before it inserts its row, so a delete that reads the
    // catalog in that window sees no reference to a shared md5 and would
    // unlink the file the new song is about to serve
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    if let Err(e) = database::delete_song(music_id) {
        return webui::error(&format!("Could not delete the song: {}", e));
    }
    database::bump_revision();
    // Global clear-rate stats for the dead live id (per-user score records are
    // wiped lazily on each user's next userdata pull)
    crate::router::clear_rate::purge_live(music_id);
    // A custom 3D MV can't outlive the song it plays over
    crate::router::custom_3dmv::purge_song(music_id);

    let _ = fs::remove_dir_all(get_data_path(&format!("custom_songs/{}", music_id)));
    // Audio is content-addressed and may be shared with another upload. A read
    // error means "assume referenced" - never unlink on a doubtful reference set
    for key in ["play", "select"] {
        let Some(md5) = cue_md5(&song["sound"][key]) else { continue; };
        if !database::audio_in_use(&md5, music_id).unwrap_or(true) {
            let _ = fs::remove_file(audio_file_path(&md5));
        }
    }
    drop(lock);

    send_json(object!{
        result: "OK"
    })
}



// Every song this account uploaded, gone - called from userdata::delete_account,
// so a purged uploader leaves no catalog row pointing at a user id that no longer
// resolves (browse renders an uploader name for every row). Runs the same steps
// the owner's own delete does, including the cross-feature MV cascade and the
// content-addressed audio GC
pub fn purge_owner(uid: i64) {
    if disabled() {
        return;
    }
    for music_id in database::music_ids_by_owner(uid) {
        let song = database::get_song(music_id).unwrap_or(object!{});
        let lock = lock_onto_mutex!(UPLOAD_LOCK);
        if database::delete_song(music_id).is_err() {
            drop(lock);
            continue;
        }
        database::bump_revision();
        crate::router::clear_rate::purge_live(music_id);
        crate::router::custom_3dmv::purge_song(music_id);
        let _ = fs::remove_dir_all(get_data_path(&format!("custom_songs/{}", music_id)));
        for key in ["play", "select"] {
            let Some(md5) = cue_md5(&song["sound"][key]) else { continue; };
            if !database::audio_in_use(&md5, music_id).unwrap_or(true) {
                let _ = fs::remove_file(audio_file_path(&md5));
            }
        }
        drop(lock);
    }
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

    // A SIF1 chart whose transcode contains 3+ simultaneous notes: 4 parallel holds into a
    // full 9-lane wall (the shape of the field-reported chart that exposed the old encoding)
    // 4 parallel holds then a 9-wide wall. Timed to fit inside the 2s test track: uploads are
    // rejected when a chart outlives its audio (validate_chart_fits_audio)
    fn wall_chart() -> Vec<u8> {
        let mut beatmap = jzon::array![];
        for position in [2, 4, 6, 8] {
            beatmap.push(jzon::object!{
                "timing_sec": 0.5, "notes_attribute": 1, "notes_level": 1,
                "effect": 3, "effect_value": 0.5, "position": position
            }).unwrap();
        }
        for position in 1..=9 {
            beatmap.push(jzon::object!{
                "timing_sec": 1.5, "notes_attribute": 1, "notes_level": 1,
                "effect": 1, "effect_value": 0.0, "position": position
            }).unwrap();
        }
        jzon::stringify(beatmap).into_bytes()
    }

    // The startup migration: a chart stored with the PRE-pairing encoding (whole equal-time
    // clusters sharing one num) is regrouped in place, its catalog md5/size follow the new
    // bytes, the revision bumps exactly once, correctly-encoded songs stay byte-identical,
    // and a second run is a complete no-op
    #[test]
    fn startup_migration_regroups_pre_fix_charts() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Migration Target");
        field(&mut fields, "artist", "Wall Artist");
        field(&mut fields, "attribute", "1");
        field(&mut fields, "level_number_4", "15");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg_tone(550.0));
        fields.insert(String::from("chart_4"), wall_chart());
        let target = create_song(3333, &fields).unwrap();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Migration Control");
        field(&mut fields, "artist", "Control Artist");
        field(&mut fields, "attribute", "2");
        field(&mut fields, "level_number_1", "5");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg_tone(770.0));
        fields.insert(String::from("chart_1"), test_chart());
        let control = create_song(4444, &fields).unwrap();

        // The upload stored the CURRENT encoding; capture it, then doctor the store back to
        // the pre-pairing form exactly as an old server would have written it: squashed
        // chart bytes on disk and the catalog md5/size matching those bytes
        let path = song_path(target, "chart_4.json");
        let fixed_bytes = fs::read(&path).unwrap();
        let mut squashed = jzon::parse(&String::from_utf8_lossy(&fixed_bytes)).unwrap();
        chart::squash_to_pre_fix(&mut squashed);
        let squashed_bytes = jzon::stringify(squashed).into_bytes();
        assert_ne!(squashed_bytes, fixed_bytes);
        fs::write(&path, &squashed_bytes).unwrap();
        let mut song = database::get_song(target).unwrap();
        let (md5, size) = asset_meta(&squashed_bytes);
        for entry in song["levels"].members_mut() {
            if entry["level"] == 4 {
                entry["md5"] = md5.clone().into();
                entry["size"] = size.into();
            }
        }
        database::update_song(target, &song);

        let control_bytes = fs::read(song_path(control, "chart_1.json")).unwrap();
        let control_song = database::get_song(control).unwrap();
        let revision = database::get_revision();

        migrate::run();

        // The target chart is byte-identical to what the current transcoder stores, and the
        // catalog follows the new bytes
        let migrated = fs::read(&path).unwrap();
        assert_eq!(migrated, fixed_bytes);
        let song = database::get_song(target).unwrap();
        let level = song["levels"].members().find(|l| l["level"] == 4).unwrap();
        let (md5, size) = asset_meta(&fixed_bytes);
        assert_eq!(level["md5"].to_string(), md5);
        assert_eq!(level["size"].as_usize().unwrap(), size);
        // full_combo never depended on grouping and must not move
        assert_eq!(level["full_combo"], 9 + 4);

        // Exactly one revision bump, and the control song is untouched
        assert_eq!(database::get_revision(), revision + 1);
        assert_eq!(fs::read(song_path(control, "chart_1.json")).unwrap(), control_bytes);
        assert_eq!(jzon::stringify(database::get_song(control).unwrap()), jzon::stringify(control_song));

        // Idempotent: a second boot changes nothing and bumps nothing
        migrate::run();
        assert_eq!(fs::read(&path).unwrap(), fixed_bytes);
        assert_eq!(database::get_revision(), revision + 1);
    }

    // The live PLAY cue must never be a loop cue: the client reports a looping playback as
    // forever-playing, and the live's end trigger waits on playback-end, so a looping play cue
    // means the live never ends. New uploads emit is_loop:false, the preview cue keeps looping,
    // and the startup migration un-loops catalogs written before the distinction existed.
    #[test]
    fn play_cue_never_loops() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Loop Check");
        field(&mut fields, "artist", "Loop Artist");
        field(&mut fields, "attribute", "1");
        field(&mut fields, "level_number_1", "5");
        fields.insert(String::from("jacket"), test_png());
        // A tone no other test uses: the audio store is content-addressed and shared across
        // tests, so a duplicate cue would keep another test's cue alive past its own GC
        fields.insert(String::from("audio"), test_ogg_tone(880.0));
        fields.insert(String::from("chart_1"), test_chart());
        let music_id = create_song(5555, &fields).unwrap();

        let song = database::get_song(music_id).unwrap();
        assert_eq!(song["sound"]["play"]["is_loop"], false);
        assert_eq!(song["sound"]["play"]["loop_end_sec"], 0.0);
        assert_eq!(song["sound"]["select"]["is_loop"], true);
        assert!(song["sound"]["select"]["loop_end_sec"].as_f64().unwrap() > 0.0);

        // Doctor the catalog back to the pre-fix shape an old server would have written,
        // then boot: the migration un-loops the play cue and bumps the revision once
        let mut old = song.clone();
        old["sound"]["play"]["is_loop"] = true.into();
        old["sound"]["play"]["loop_end_sec"] = old["sound"]["play"]["duration_sec"].clone();
        database::update_song(music_id, &old);
        let revision = database::get_revision();

        migrate::run();

        let song = database::get_song(music_id).unwrap();
        assert_eq!(song["sound"]["play"]["is_loop"], false);
        assert_eq!(song["sound"]["play"]["loop_end_sec"], 0.0);
        assert_eq!(song["sound"]["select"]["is_loop"], true);
        // The ogg bytes never moved, so the audio md5 must not change (no re-download)
        assert_eq!(song["sound"]["play"]["md5"], old["sound"]["play"]["md5"]);
        assert_eq!(database::get_revision(), revision + 1);

        // Idempotent
        migrate::run();
        assert_eq!(database::get_revision(), revision + 1);
    }

    // Song text is rendered by TMP with rich text on and no escaping, so a tag in a name or an
    // artist is rejected at upload and at edit. A '<' that TMP wouldn't read as a tag survives.
    #[test]
    fn song_text_may_not_carry_rich_text_tags() {
        let _lock = crate::runtime::lock_test_data_path();

        let base = || {
            let mut fields = HashMap::new();
            field(&mut fields, "name", "Tag Check");
            field(&mut fields, "artist", "Tag Artist");
            field(&mut fields, "attribute", "1");
            field(&mut fields, "level_number_1", "5");
            fields.insert(String::from("jacket"), test_png());
            fields.insert(String::from("audio"), test_ogg_tone(1210.0));
            fields.insert(String::from("chart_1"), test_chart());
            fields
        };

        for (key, label) in [
            ("name", "Song name"),
            ("name_en", "Song English name"),
            ("short_name", "Short name"),
            ("kana", "Name reading"),
            ("artist", "Artist"),
            ("artist_en", "English artist")
        ] {
            let mut fields = base();
            field(&mut fields, key, "<size=400%>boom");
            let error = create_song(8888, &fields).unwrap_err();
            assert!(error.contains(label), "{} -> {}", key, error);
            assert!(error.contains("<size>"), "{} -> {}", key, error);
            assert!(database::get_song(8888).is_none());
        }

        // "<3" is not a tag, so it uploads
        let mut fields = base();
        field(&mut fields, "name", "I <3 LIVE");
        let music_id = create_song(8888, &fields).unwrap();
        assert_eq!(database::get_song(music_id).unwrap()["name"], "I <3 LIVE");

        // Edits are held to the same rule, and the stored song survives the rejection
        let before = database::get_song(music_id).unwrap();
        let mut fields = HashMap::new();
        field(&mut fields, "artist", "<sprite=1>");
        assert!(update_song(music_id, &fields).unwrap_err().contains("Artist"));
        assert_eq!(jzon::stringify(database::get_song(music_id).unwrap()), jzon::stringify(before));
    }

    // The catalog the GAME reads carries the uploader's account name, which the client turns
    // into the song's detail-info credit line. Account names are stored verbatim by the profile
    // route, so the catalog strips rich-text tags out of them
    #[test]
    fn the_game_catalog_carries_a_tag_free_uploader_name() {
        let _lock = crate::runtime::lock_test_data_path();

        let uid = 1;
        let mut fields = HashMap::new();
        field(&mut fields, "name", "Credited");
        field(&mut fields, "artist", "Credit Artist");
        field(&mut fields, "attribute", "2");
        field(&mut fields, "level_number_1", "5");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg_tone(1320.0));
        fields.insert(String::from("chart_1"), test_chart());
        let music_id = create_song(uid, &fields).unwrap();

        let songs = database::get_songs_for_user(uid);
        let song = songs.members().find(|s| s["music_id"] == music_id).unwrap();
        // The router adds the field; the stored blob never holds it
        assert!(song["uploader"].is_null());

        // The tag stripper is what the router applies to the account name
        assert_eq!(rich_text::strip_tags("<size=400%>Nozomi"), "Nozomi");
        assert_eq!(rich_text::strip_tags("Honoka"), "Honoka");
    }

    // A chart that outlives its audio is rejected on upload AND on edit (from either side -
    // swapping in a longer chart, or shorter audio under charts that stay). The live ends when
    // the music does, so those notes would never be judged. A note at t=0 is fine: the 2.0s
    // count-in always covers the marker's flight time.
    #[test]
    fn chart_must_fit_its_audio() {
        let _lock = crate::runtime::lock_test_data_path();

        // Taps at the given times, one per lane sweep
        let chart_at = |times: &[f64]| {
            let mut beatmap = jzon::array![];
            for (i, time) in times.iter().enumerate() {
                beatmap.push(jzon::object!{
                    "timing_sec": *time, "notes_attribute": 1, "notes_level": 1,
                    "effect": 1, "effect_value": 0.0, "position": (i % 9) + 1
                }).unwrap();
            }
            jzon::stringify(beatmap).into_bytes()
        };
        let base = |chart: Vec<u8>| {
            let mut fields = HashMap::new();
            field(&mut fields, "name", "Fit Check");
            field(&mut fields, "artist", "Fit Artist");
            field(&mut fields, "attribute", "1");
            field(&mut fields, "level_number_1", "5");
            fields.insert(String::from("jacket"), test_png());
            // 2 seconds of audio
            fields.insert(String::from("audio"), test_ogg_tone(990.0));
            fields.insert(String::from("chart_1"), chart);
            fields
        };

        // 2.5s note in a 2.0s track: the last note's MISS window closes long after the live ends
        let error = create_song(6666, &base(chart_at(&[0.5, 2.5]))).unwrap_err();
        assert!(error.contains("Difficulty 1"), "{}", error);
        assert!(error.contains("the audio is only"), "{}", error);
        // Nothing was stored
        assert!(database::get_song(6666).is_none());

        // Right at the edge: 1.8 + the 0.15 tap MISS window is 1.95, inside 2.0. A note at t=0
        // is accepted too - the count-in covers the marker flight
        let music_id = create_song(6666, &base(chart_at(&[0.0, 1.8]))).unwrap();
        let before = database::get_song(music_id).unwrap();

        // Editing in a chart that doesn't fit the stored audio is rejected, and the stored
        // song is untouched
        let mut fields = HashMap::new();
        fields.insert(String::from("chart_1"), chart_at(&[0.5, 3.0]));
        let error = update_song(music_id, &fields).unwrap_err();
        assert!(error.contains("the audio is only"), "{}", error);
        assert_eq!(jzon::stringify(database::get_song(music_id).unwrap()), jzon::stringify(before.clone()));

        // Adding a difficulty whose chart doesn't fit is rejected the same way
        let mut fields = HashMap::new();
        fields.insert(String::from("chart_4"), chart_at(&[0.5, 5.0]));
        field(&mut fields, "level_number_4", "12");
        assert!(update_song(music_id, &fields).is_err());
        assert_eq!(jzon::stringify(database::get_song(music_id).unwrap()), jzon::stringify(before));

        // A chart that DOES fit still edits in
        let mut fields = HashMap::new();
        fields.insert(String::from("chart_1"), chart_at(&[0.25, 1.5]));
        update_song(music_id, &fields).unwrap();
    }

    // The startup migration also corrects the two fabricated masterdata values in stored
    // catalogs: score-rank thresholds (official constants, not per-song) and combo missions
    // (20/40/60/80% of the hardest full combo, not 25/50/75/100%)
    #[test]
    fn startup_migration_fixes_fabricated_scores_and_missions() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Old Values");
        field(&mut fields, "artist", "Old Artist");
        field(&mut fields, "attribute", "3");
        field(&mut fields, "level_number_1", "5");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg_tone(1100.0));
        fields.insert(String::from("chart_1"), wall_chart());
        let music_id = create_song(7777, &fields).unwrap();

        let mut song = database::get_song(music_id).unwrap();
        let hardest = song["levels"].members().last().unwrap()["full_combo"].as_i64().unwrap();
        // Doctor the catalog back to the pre-fix formulas
        let base = hardest as f64 * 200.0 * (1.0 + 5.0 / 10.0);
        song["score"] = object!{
            "c": (base * 0.5) as u32, "b": (base * 0.75) as u32, "a": base as u32, "s": (base * 1.3) as u32
        };
        song["multi_score"] = object!{
            "c": (base * 0.6) as u32, "b": (base * 0.9) as u32, "a": (base * 1.2) as u32, "s": (base * 1.56) as u32
        };
        song["mission_combo"] = jzon::array![hardest / 4, hardest / 2, hardest * 3 / 4, hardest];
        database::update_song(music_id, &song);
        let revision = database::get_revision();

        migrate::run();

        let song = database::get_song(music_id).unwrap();
        let (score, multi_score) = default_scores();
        assert_eq!(song["score"], score);
        assert_eq!(song["multi_score"], multi_score);
        assert_eq!(song["score"]["s"], 350000);
        assert_eq!(song["multi_score"]["s"], 1225000);
        // 20/40/60/80%, and never the full combo itself
        assert_eq!(song["mission_combo"], mission_combo(hardest));
        assert_eq!(song["mission_combo"][3].as_i64().unwrap(), (hardest as f64 * 0.8 + 0.5) as i64);
        assert!(song["mission_combo"][3].as_i64().unwrap() < hardest);
        assert_eq!(database::get_revision(), revision + 1);

        // Idempotent
        migrate::run();
        assert_eq!(database::get_revision(), revision + 1);
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

        // Adding a difficulty re-derives the combo missions from the new hardest chart, the
        // score thresholds stay on the official constants, and the manifest follows so
        // exports reflect the edited state
        let mut fields = HashMap::new();
        fields.insert(String::from("chart_4"), test_chart());
        field(&mut fields, "level_number_4", "12");
        update_song(music_id, &fields).unwrap();
        let song = database::get_song(music_id).unwrap();
        assert_eq!(song["levels"].len(), 2);
        let (score, _) = default_scores();
        assert_eq!(song["score"]["s"], score["s"]);
        let hardest = song["levels"].members().last().unwrap()["full_combo"].as_i64().unwrap();
        assert_eq!(song["mission_combo"], mission_combo(hardest));
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

    // The startup sweep drops oggs nothing references any more (an upload that
    // died between writing the file and storing its row), keeps every cue that
    // is referenced - including one shared by two songs - and leaves anything
    // that isn't a content-addressed ogg alone
    #[test]
    fn audio_sweep_removes_only_orphans() {
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Sweep Song");
        field(&mut fields, "artist", "Sweep Artist");
        field(&mut fields, "attribute", "1");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg_tone(311.0));
        fields.insert(String::from("chart_1"), test_chart());
        let music_id = create_song(4242, &fields).unwrap();
        // A second song on the SAME audio: identical bytes hash to one shared file
        let shared_id = create_song(4243, &fields).unwrap();
        let song = database::get_song(music_id).unwrap();
        let play = song["sound"]["play"]["md5"].to_string();
        let select = song["sound"]["select"]["md5"].to_string();
        assert_eq!(database::get_song(shared_id).unwrap()["sound"]["play"]["md5"], play);

        let orphan = audio_file_path(&"a1".repeat(16));
        let not_an_ogg = get_data_path("custom_songs/audio/notes.txt");
        let bad_name = get_data_path("custom_songs/audio/orphan.ogg");
        for path in [&orphan, &not_an_ogg, &bad_name] {
            fs::write(path, b"junk").unwrap();
        }

        sweep_audio();

        assert!(fs::read(&orphan).is_err());
        assert!(fs::read(audio_file_path(&play)).is_ok());
        assert!(fs::read(audio_file_path(&select)).is_ok());
        // Only {32 hex}.ogg is ever a sweep candidate
        assert!(fs::read(&not_an_ogg).is_ok());
        assert!(fs::read(&bad_name).is_ok());

        // A song whose cues can't be read makes the whole sweep bail rather
        // than treat that song as referencing nothing
        fs::write(&orphan, b"junk").unwrap();
        let shared_song = database::get_song(shared_id).unwrap();
        database::update_song(shared_id, &object!{"music_id": shared_id});
        sweep_audio();
        assert!(fs::read(&orphan).is_ok());
        assert!(fs::read(audio_file_path(&play)).is_ok());

        database::update_song(shared_id, &shared_song);
        let _ = fs::remove_file(&orphan);
        let _ = fs::remove_file(&not_an_ogg);
        let _ = fs::remove_file(&bad_name);
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
        database::insert_song(public_id, owner, &object!{music_id: public_id}, "public", &array![], false).unwrap();
        let private_id = database::next_music_id();
        database::insert_song(private_id, owner, &object!{music_id: private_id}, "private", &array![], false).unwrap();
        let shared_id = database::next_music_id();
        database::insert_song(shared_id, owner, &object!{music_id: shared_id}, "shared", &array![friend], false).unwrap();

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
        database::insert_song(locked_id, owner, &object!{music_id: locked_id}, "public", &array![], true).unwrap();
        let open_id = database::next_music_id();
        database::insert_song(open_id, owner, &object!{music_id: open_id}, "public", &array![], false).unwrap();
        let private_id = database::next_music_id();
        database::insert_song(private_id, owner, &object!{music_id: private_id}, "private", &array![], false).unwrap();

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
    // for clients whose X-Protocol-Version is >= 1. An old/official client (no
    // header, or a non-numeric value) gets its official unlock list untouched.
    #[test]
    fn unlock_ids_gated_on_protocol_version() {
        use actix_web::test::TestRequest;
        let _lock = crate::runtime::lock_test_data_path();

        let owner = 4242;
        let music_id = database::next_music_id();
        database::insert_song(music_id, owner, &object!{music_id: music_id}, "public", &array![], false).unwrap();
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

        // No header -> protocol version 0 -> nothing appended
        let without = TestRequest::default().to_http_request();
        assert!(!client_supports_custom_songs(&without));
        assert!(appended(&without).is_empty());

        // Version 1 or higher -> supported -> custom id appended
        let with = TestRequest::default().insert_header(("X-Protocol-Version", "1")).to_http_request();
        assert!(client_supports_custom_songs(&with));
        assert!(appended(&with).contains(music_id));
        let newer = TestRequest::default().insert_header(("X-Protocol-Version", "7")).to_http_request();
        assert!(client_supports_custom_songs(&newer));

        // A non-numeric value is treated as version 0
        let wrong = TestRequest::default().insert_header(("X-Protocol-Version", "true")).to_http_request();
        assert!(!client_supports_custom_songs(&wrong));
        assert!(appended(&wrong).is_empty());
    }

    // Catalog chart/jacket entries carry md5+size, and the content-addressed
    // data route serves the exact bytes for a known md5 (404 for unknown / off)
    #[test]
    fn catalog_md5_and_data_route() {
        use actix_web::test::TestRequest;
        let _lock = crate::runtime::lock_test_data_path();

        let mut fields = HashMap::new();
        field(&mut fields, "name", "Data Route");
        field(&mut fields, "artist", "A");
        field(&mut fields, "attribute", "1");
        fields.insert(String::from("jacket"), test_png());
        fields.insert(String::from("audio"), test_ogg());
        // A chart UNIQUE to this test: the self-heal assert below relies on
        // the old md5 resolving nowhere, and the shared test_chart() bytes
        // also live in other tests' songs (the md5 index is content-addressed
        // across all songs, so identical charts alias)
        fields.insert(String::from("chart_1"), jzon::stringify(jzon::array![
            {"timing_sec": 0.25, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 2},
            {"timing_sec": 0.75, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 6},
            {"timing_sec": 1.25, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 9}
        ]).into_bytes());
        let music_id = create_song(9191, &fields).unwrap();

        let song = database::get_song(music_id).unwrap();
        let chart_md5 = song["levels"][0]["md5"].to_string();
        let jacket_md5 = song["jacket_md5"].to_string();
        let blur_md5 = song["jacket_blur_md5"].to_string();
        // Every downloadable asset entry carries a 32-hex md5 and a nonzero size
        for (md5, size) in [
            (&chart_md5, song["levels"][0]["size"].as_usize()),
            (&jacket_md5, song["jacket_size"].as_usize()),
            (&blur_md5, song["jacket_blur_size"].as_usize())
        ] {
            assert_eq!(md5.len(), 32, "md5 {}", md5);
            assert!(md5.chars().all(|c| c.is_ascii_hexdigit()));
            assert!(size.unwrap() > 0);
        }
        // Audio cues still carry md5+size (unchanged)
        assert_eq!(song["sound"]["play"]["md5"].to_string().len(), 32);
        assert!(song["sound"]["select"]["size"].as_usize().unwrap() > 0);

        let call = |hash: &str, ext: &str| -> HttpResponse {
            let req = TestRequest::default()
                .param("hash", hash.to_string())
                .param("file", format!("{}.{}", hash, ext))
                .to_http_request();
            actix_web::rt::System::new().block_on(async { data(req).await })
        };
        let body_of = |resp: HttpResponse| -> Vec<u8> {
            actix_web::rt::System::new()
                .block_on(async { actix_web::body::to_bytes(resp.into_body()).await.unwrap() })
                .to_vec()
        };

        // The md5 route serves the exact bytes whose hash IS that md5
        let resp = call(&chart_md5, "json");
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        let body = body_of(resp);
        assert_eq!(format!("{:x}", md5::compute(&body)), chart_md5);
        assert_eq!(body.len(), song["levels"][0]["size"].as_usize().unwrap());

        let resp = call(&jacket_md5, "png");
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        assert_eq!(format!("{:x}", md5::compute(body_of(resp))), jacket_md5);

        let resp = call(&blur_md5, "png");
        assert_eq!(resp.status(), actix_web::http::StatusCode::OK);
        assert_eq!(format!("{:x}", md5::compute(body_of(resp))), blur_md5);

        // Unknown md5 -> 404
        assert_eq!(call(&"0".repeat(32), "json").status(), actix_web::http::StatusCode::NOT_FOUND);
        // Malformed hash -> 404
        assert_eq!(call("nothex", "json").status(), actix_web::http::StatusCode::NOT_FOUND);

        // Feature disabled -> 404 even for a real md5
        crate::runtime::set_enable_custom_songs(false);
        assert_eq!(call(&chart_md5, "json").status(), actix_web::http::StatusCode::NOT_FOUND);
        crate::runtime::set_enable_custom_songs(true);

        // Editing the chart changes its md5 (self-heal): old md5 stops resolving
        let mut edit = HashMap::new();
        edit.insert(String::from("chart_1"), jzon::stringify(jzon::array![
            {"timing_sec": 0.5, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 4},
            {"timing_sec": 1.2, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 8}
        ]).into_bytes());
        update_song(music_id, &edit).unwrap();
        let new_md5 = database::get_song(music_id).unwrap()["levels"][0]["md5"].to_string();
        assert_ne!(new_md5, chart_md5);
        assert_eq!(call(&chart_md5, "json").status(), actix_web::http::StatusCode::NOT_FOUND);
        assert_eq!(call(&new_md5, "json").status(), actix_web::http::StatusCode::OK);
    }

    // The public clear-rate HTML page shows the REAL title for public custom
    // songs (escaped - names are user input), keeps private songs entirely
    // absent, and prefers name_en for the EN title attribute
    #[test]
    fn clearrate_html_shows_public_custom_song_titles() {
        use actix_web::test::TestRequest;
        use crate::router::clear_rate;
        let _lock = crate::runtime::lock_test_data_path();

        let public_id = database::next_music_id();
        database::insert_song(public_id, 6100, &object!{
            music_id: public_id,
            name: "Public <Song> & \"Co\"",
            name_en: "Public Song EN"
        }, "public", &array![], false).unwrap();
        let private_id = database::next_music_id();
        database::insert_song(private_id, 6100, &object!{
            music_id: private_id,
            name: "Top Secret Anthem"
        }, "private", &array![], false).unwrap();
        for id in [public_id, private_id] {
            clear_rate::live_completed(id, 1, false, 100, 6100);
        }
        clear_rate::invalidate_cache();

        let html = actix_web::rt::System::new().block_on(async {
            let resp = clear_rate::clearrate_html(TestRequest::default().to_http_request()).await;
            let body = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
            String::from_utf8_lossy(&body).to_string()
        });
        // The public song's real (escaped) title, JP cell and EN attribute
        assert!(html.contains("Public &lt;Song&gt; &amp; &quot;Co&quot;"), "public title missing");
        assert!(html.contains("Public Song EN"), "EN title missing");
        // Never the raw unescaped markup
        assert!(!html.contains("Public <Song>"), "title not escaped");
        // The private song leaks neither name nor row
        assert!(!html.contains("Top Secret Anthem"), "private name leaked");

        // The title lookup itself only answers for public songs
        assert_eq!(database::public_song_title(public_id, false), Some(String::from("Public <Song> & \"Co\"")));
        assert_eq!(database::public_song_title(public_id, true), Some(String::from("Public Song EN")));
        assert_eq!(database::public_song_title(private_id, false), None);
        assert_eq!(database::public_song_title(private_id + 5000, false), None);
    }

    // The JSON clear-rate endpoint filters non-public custom songs per requesting
    // user: the owner sees all of theirs, a shared user sees the songs shared with
    // them, everyone else sees only public ones. Official (stock) live ids are
    // always visible. The parallel master_music_ids array must stay index-aligned
    // with all_user_clear_rate after filtering.
    #[test]
    fn clearrate_hides_custom_songs_per_user() {
        use actix_web::{test::TestRequest, Responder};
        use crate::router::clear_rate;
        let _lock = crate::runtime::lock_test_data_path();
        crate::runtime::set_enable_custom_songs(true);

        let owner = 5001;
        let shared_user = 5002;
        let outsider = 5003;

        let public_id = database::next_music_id();
        database::insert_song(public_id, owner, &object!{music_id: public_id}, "public", &array![], false).unwrap();
        let private_id = database::next_music_id();
        database::insert_song(private_id, owner, &object!{music_id: private_id}, "private", &array![], false).unwrap();
        let shared_id = database::next_music_id();
        database::insert_song(shared_id, owner, &object!{music_id: shared_id}, "shared", &array![shared_user], false).unwrap();
        // A stock live id, outside the custom range - never filtered
        let stock_id: i64 = 1_500_123;

        for id in [public_id, private_id, shared_id, stock_id] {
            clear_rate::live_completed(id, 1, false, 100, owner);
        }
        clear_rate::invalidate_cache();

        // master_live_ids the endpoint serves to this uid, with an index-alignment guard
        let visible_to = |uid: i64| -> Vec<i64> {
            let req = TestRequest::default().insert_header(("aoharu-user-id", uid.to_string())).to_http_request();
            let body = actix_web::rt::System::new().block_on(async {
                let resp = clear_rate::clearrate(req.clone()).await.respond_to(&req).map_into_boxed_body();
                actix_web::body::to_bytes(resp.into_body()).await.unwrap()
            });
            let json = jzon::parse(&crate::encryption::decrypt_packet(&String::from_utf8_lossy(&body)).unwrap()).unwrap();
            let rates = &json["data"]["all_user_clear_rate"];
            let ids = &json["data"]["master_music_ids"];
            assert_eq!(rates.len(), ids.len(), "parallel arrays must stay aligned for uid {}", uid);
            rates.members().map(|r| r["master_live_id"].as_i64().unwrap()).collect()
        };
        let sees = |uid: i64, id: i64| visible_to(uid).contains(&id);

        // Owner sees every song of theirs plus the stock id
        assert!(sees(owner, public_id) && sees(owner, private_id) && sees(owner, shared_id) && sees(owner, stock_id));
        // Shared user sees public + shared + stock, never the private one
        assert!(sees(shared_user, public_id) && sees(shared_user, shared_id) && sees(shared_user, stock_id));
        assert!(!sees(shared_user, private_id));
        // Outsider and anonymous (uid 0) see public + stock only
        for uid in [outsider, 0] {
            assert!(sees(uid, public_id) && sees(uid, stock_id));
            assert!(!sees(uid, private_id) && !sees(uid, shared_id));
        }
    }

    // ---- defect-fix coverage -------------------------------------------------

    use actix_web::test::TestRequest;
    use std::io::Write;

    // A real multipart body, so the streaming caps in read_multipart are exercised
    // by the reader itself rather than by a hand-built field map
    async fn multipart_of(parts: Vec<(&str, Vec<u8>)>) -> Multipart {
        let boundary = "ewtestboundary";
        let mut body: Vec<u8> = Vec::new();
        for (name, data) in parts {
            body.extend(format!(
                "--{}\r\nContent-Disposition: form-data; name=\"{}\"; filename=\"{}\"\r\nContent-Type: application/octet-stream\r\n\r\n",
                boundary, name, name
            ).into_bytes());
            body.extend(data);
            body.extend(b"\r\n");
        }
        body.extend(format!("--{}--\r\n", boundary).into_bytes());
        let (req, mut payload) = TestRequest::default()
            .insert_header(("content-type", format!("multipart/form-data; boundary={}", boundary)))
            .set_payload(actix_web::web::Bytes::from(body))
            .to_http_parts();
        <Multipart as actix_web::FromRequest>::from_request(&req, &mut payload).await.unwrap()
    }

    // A real webui session for `uid`, the way the browser gets one
    fn webui_session(auth_token: &str) -> (i64, String) {
        let uid = userdata::get_acc(auth_token)["user"]["id"].as_i64().unwrap();
        userdata::user::migration::save_acc_transfer(uid, "hunter2");
        (uid, userdata::webui_login(uid, "hunter2").unwrap())
    }

    // A jacket whose processed bytes are unique to this seed. The md5 index is
    // content-addressed ACROSS songs, so a shared test_png() would let one test's
    // jacket md5 resolve to another test's file
    fn seeded_png(seed: u8) -> Vec<u8> {
        let mut rv = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(64, 32, |x, y| {
            image::Rgba([(x * 4) as u8, (y * 8) as u8, seed, 255])
        })).write_to(&mut std::io::Cursor::new(&mut rv), image::ImageFormat::Png).unwrap();
        rv
    }

    fn song_fields(name: &str, tone: f32) -> HashMap<String, Vec<u8>> {
        let mut fields = HashMap::new();
        field(&mut fields, "name", name);
        field(&mut fields, "artist", "A");
        field(&mut fields, "attribute", "1");
        fields.insert(String::from("jacket"), seeded_png(tone as u8));
        fields.insert(String::from("audio"), test_ogg_tone(tone));
        fields.insert(String::from("chart_1"), test_chart());
        fields
    }

    // Renaming the table away is the cheapest way to make every query against it
    // fail the way a busy/corrupt database does, without losing the rows
    fn with_songs_table_broken<T>(body: impl FnOnce() -> T) -> T {
        let conn = rusqlite::Connection::open(database::test_db_path()).unwrap();
        conn.execute("ALTER TABLE songs RENAME TO songs_hidden", ()).unwrap();
        let rv = body();
        conn.execute("ALTER TABLE songs_hidden RENAME TO songs", ()).unwrap();
        rv
    }

    // D6: the multipart reader caps a field WHILE it streams, before any byte
    // reaches the audio/png/chart parsers. Every other test builds the field map
    // directly, so this is the only one that goes through the reader itself
    #[test]
    fn the_multipart_reader_caps_an_oversize_field() {
        let _lock = crate::runtime::lock_test_data_path();
        actix_web::rt::System::new().block_on(async {
            let oversize = vec![b'a'; MAX_FILE_BYTES + 1];
            let err = read_multipart(multipart_of(vec![("audio", oversize)]).await).await.unwrap_err();
            assert!(err.contains("'audio'") && err.contains("per-file limit"), "{}", err);

            // A body inside the caps still arrives intact
            let fields = read_multipart(multipart_of(vec![
                ("name", b"Capped".to_vec()),
                ("jacket", test_png())
            ]).await).await.unwrap();
            assert_eq!(field_str(&fields, "name"), "Capped");
            assert_eq!(fields.get("jacket"), Some(&test_png()));
        });
    }

    // D1/D6: the per-request total is accounted over the whole form, which is also
    // what a package's expanded contents are re-checked against
    #[test]
    fn the_request_total_is_capped() {
        let mut fields: HashMap<String, Vec<u8>> = HashMap::new();
        fields.insert(String::from("a"), vec![0; MAX_FILE_BYTES]);
        fields.insert(String::from("b"), vec![0; MAX_FILE_BYTES]);
        assert!(check_field_caps(&fields).is_ok());
        fields.insert(String::from("c"), vec![0; 1]);
        let err = check_field_caps(&fields).unwrap_err();
        assert!(err.contains("per-request limit"), "{}", err);

        let mut one = HashMap::new();
        one.insert(String::from("audio"), vec![0; MAX_FILE_BYTES + 1]);
        assert!(check_field_caps(&one).unwrap_err().contains("per-file limit"));
    }

    // D1: a package entry is capped as it inflates. Deflate's ~1032:1 ceiling
    // means an uncapped read_to_end here turns a tiny zip into gigabytes
    #[test]
    fn package_import_is_capped() {
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(jzon::stringify(object!{
            "format": 1, "name": "Bomb", "artist": "A", "attribute": 1,
            "levels": [{ "level": 1, "level_number": 3 }]
        }).as_bytes()).unwrap();
        zip.start_file("jacket", options).unwrap();
        zip.write_all(&test_png()).unwrap();
        zip.start_file("chart_1.json", options).unwrap();
        zip.write_all(&test_chart()).unwrap();
        // Compresses to a few KB, inflates to just over the per-file cap
        zip.start_file("audio", options).unwrap();
        zip.write_all(&vec![0u8; MAX_FILE_BYTES + 1]).unwrap();
        let package = zip.finish().unwrap().into_inner();
        assert!(package.len() < 1024 * 1024, "the bomb should be small: {} bytes", package.len());

        let mut fields = HashMap::new();
        let err = package::expand(&package, &mut fields).unwrap_err();
        assert!(err.contains("per-file limit"), "{}", err);
        // Nothing oversized was buffered into the field map
        assert!(fields.get("audio").is_none());
    }

    // D3: /custom_song/assets/{music_id}/{file} is addressed by a SEQUENTIAL id,
    // not by a content hash, so it gets the catalog's visibility rule: the whole
    // private/shared catalog used to be walkable from music_id 10000 up
    #[test]
    fn the_asset_route_is_visibility_gated() {
        let _lock = crate::runtime::lock_test_data_path();
        let (owner, owner_cookie) = webui_session("custom-song-assets-owner");
        let (_, stranger_cookie) = webui_session("custom-song-assets-stranger");

        let public_id = create_song(owner, &song_fields("Assets Public", 331.0)).unwrap();
        let private_id = create_song(owner, &song_fields("Assets Private", 337.0)).unwrap();
        database::set_visibility(private_id, "private", &array![]).unwrap();

        let call = |music_id: i64, cookie: Option<&str>| -> HttpResponse {
            let mut req = TestRequest::default()
                .param("music_id", music_id.to_string())
                .param("file", String::from("jacket.png"));
            if let Some(cookie) = cookie {
                req = req.insert_header(("Cookie", format!("ew_token={}", cookie)));
            }
            let req = req.to_http_request();
            actix_web::rt::System::new().block_on(async { assets(req).await })
        };
        let ok = actix_web::http::StatusCode::OK;
        let missing = actix_web::http::StatusCode::NOT_FOUND;

        // Public: everyone, session or not
        assert_eq!(call(public_id, None).status(), ok);
        assert_eq!(call(public_id, Some(&stranger_cookie)).status(), ok);
        // Private: the owner only
        assert_eq!(call(private_id, Some(&owner_cookie)).status(), ok);
        assert_eq!(call(private_id, None).status(), missing);
        assert_eq!(call(private_id, Some(&stranger_cookie)).status(), missing);

        // Shared: the owner plus the shared list
        let stranger = userdata::get_acc(&userdata::webui_login_token(&stranger_cookie).unwrap())["user"]["id"].as_i64().unwrap();
        database::set_visibility(private_id, "shared", &array![stranger]).unwrap();
        assert_eq!(call(private_id, Some(&stranger_cookie)).status(), ok);
        assert_eq!(call(private_id, None).status(), missing);

        purge_owner(owner);
    }

    // D14: jackets and charts live at fixed per-song filenames that an in-place
    // edit overwrites, so the md5 index can briefly point at bytes that are no
    // longer its own. The client caches by md5 and never re-checks, so the route
    // verifies before it serves
    #[test]
    fn the_data_route_refuses_bytes_that_do_not_match_the_md5() {
        let _lock = crate::runtime::lock_test_data_path();
        let music_id = create_song(7710, &song_fields("Mismatch", 349.0)).unwrap();
        let jacket_md5 = database::get_song(music_id).unwrap()["jacket_md5"].to_string();

        let call = || -> HttpResponse {
            let req = TestRequest::default()
                .param("hash", jacket_md5.clone())
                .param("file", format!("{}.png", jacket_md5))
                .to_http_request();
            actix_web::rt::System::new().block_on(async { data(req).await })
        };
        assert_eq!(call().status(), actix_web::http::StatusCode::OK);

        // The index still resolves, but the file no longer holds those bytes
        let real = fs::read(song_path(music_id, "jacket.png")).unwrap();
        let mut different = real.clone();
        different.extend(b"not the bytes that hash to that md5");
        fs::write(song_path(music_id, "jacket.png"), &different).unwrap();
        assert_eq!(call().status(), actix_web::http::StatusCode::NOT_FOUND);

        fs::write(song_path(music_id, "jacket.png"), &real).unwrap();
        assert_eq!(call().status(), actix_web::http::StatusCode::OK);
        purge_owner(7710);
    }

    // D5: the chart transcoder is linear per note and its input is only bounded in
    // bytes, so the note count has its own ceiling - and the duplicate check that
    // used to rescan every preceding note still rejects what it always did
    #[test]
    fn chart_size_is_bounded_and_duplicates_still_rejected() {
        let mut too_many = jzon::array![];
        for i in 0..(chart::MAX_NOTES + 1) {
            too_many.push(object!{
                "timing_sec": 1.0 + i as f64 * 0.001, "notes_attribute": 1, "notes_level": 1,
                "effect": 1, "effect_value": 0.0, "position": 5
            }).unwrap();
        }
        let err = chart::transcode(&too_many).unwrap_err();
        assert!(err.contains("the maximum is"), "{}", err);

        // A large but legal chart still transcodes (and does so in linear time)
        let mut big = jzon::array![];
        for i in 0..5000 {
            big.push(object!{
                "timing_sec": 1.0 + i as f64 * 0.01, "notes_attribute": 1, "notes_level": 1,
                "effect": 1, "effect_value": 0.0, "position": (i % 9) + 1
            }).unwrap();
        }
        let (_, combo) = chart::transcode(&big).unwrap();
        assert_eq!(combo, 5000);

        // Same timing + position with a different effect is still a rejection
        let clash = jzon::array![
            {"timing_sec": 1.0, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 4},
            {"timing_sec": 1.0, "notes_attribute": 1, "notes_level": 1, "effect": 3, "effect_value": 0.5, "position": 4}
        ];
        assert!(chart::transcode(&clash).unwrap_err().contains("duplicate timing"));

        // ...and a non-finite timing is refused instead of panicking the
        // time-order sort's partial_cmp().unwrap()
        let nan = jzon::array![
            {"timing_sec": f64::INFINITY, "notes_attribute": 1, "notes_level": 1, "effect": 1, "effect_value": 0.0, "position": 4}
        ];
        assert!(chart::transcode(&nan).unwrap_err().contains("finite"));
    }

    // D2/D8: a read error must never read as "no rows". The GC unlinks on
    // "not referenced", so a failed lookup has to mean "assume referenced" - and a
    // failed write has to reach the uploader as an error, not as a worker panic
    #[test]
    fn a_database_error_never_deletes_audio_or_panics() {
        let _lock = crate::runtime::lock_test_data_path();
        let music_id = create_song(7711, &song_fields("Fail Closed", 353.0)).unwrap();
        let play = database::get_song(music_id).unwrap()["sound"]["play"]["md5"].to_string();
        assert_eq!(database::audio_in_use(&play, 0), Ok(true));
        assert_eq!(database::audio_in_use(&"c3".repeat(16), 0), Ok(false));

        with_songs_table_broken(|| {
            // Fail-closed: an error, not a "false" that would unlink a live file
            assert!(database::audio_in_use(&play, 0).is_err());
            assert!(database::audio_in_use(&play, 0).unwrap_or(true));

            // And an insert that cannot land is an error response, not a panic
            let err = create_song(7711, &song_fields("Never Stored", 359.0)).unwrap_err();
            assert!(err.contains("Could not store the song"), "{}", err);
        });

        assert!(fs::read(audio_file_path(&play)).is_ok(), "the live ogg was unlinked");
        purge_owner(7711);
    }

    // D15: the quota counts the bytes the catalog quotes to the client, per owner
    #[test]
    fn uploads_are_bounded_by_a_per_account_byte_quota() {
        let _lock = crate::runtime::lock_test_data_path();
        purge_owner(7712);
        assert_eq!(database::owner_bytes(7712, 0), 0);

        let music_id = create_song(7712, &song_fields("Quota", 367.0)).unwrap();
        let song = database::get_song(music_id).unwrap();
        let stored = database::song_bytes(&song);
        assert!(stored > 0);
        assert_eq!(database::owner_bytes(7712, 0), stored);
        // An in-place edit replaces its own bytes rather than adding to them
        assert_eq!(database::owner_bytes(7712, music_id), 0);

        assert!(check_quota(7712, 1, 0).is_ok());
        let err = check_quota(7712, MAX_BYTES_PER_USER, 0).unwrap_err();
        assert!(err.contains("per-account limit"), "{}", err);
        // Another account's uploads are not on this one's bill
        assert_eq!(database::owner_bytes(7713, 0), 0);

        purge_owner(7712);
    }

    // D10: purging an account takes its uploads in all three features with it -
    // otherwise every catalog keeps rows whose owner_id no longer resolves, and
    // browse renders an uploader name for each of them
    #[test]
    fn deleting_an_account_purges_its_uploads() {
        let _lock = crate::runtime::lock_test_data_path();
        let auth = "custom-content-purge-token";
        let uid = userdata::get_acc(auth)["user"]["id"].as_i64().unwrap();

        let music_id = create_song(uid, &song_fields("Purged", 373.0)).unwrap();
        let dir = get_data_path(&format!("custom_songs/{}", music_id));
        let play = database::get_song(music_id).unwrap()["sound"]["play"]["md5"].to_string();

        let mv_id = crate::database::custom_3dmv::next_mv_id();
        crate::database::custom_3dmv::insert_mv(mv_id, music_id, uid, &object!{
            "mv_id": mv_id, "music_id": music_id, "name": "Purged MV", "member_count": 1, "files": []
        }, true).unwrap();
        let card_id = crate::database::custom_card::next_card_id();
        crate::database::custom_card::insert_card(card_id, 1001, uid, &object!{
            "master_card_id": card_id, "rarity": 1
        }, true, true).unwrap();
        let character_id = crate::database::custom_card::next_character_id();
        crate::database::custom_card::insert_character(character_id, uid, &object!{
            "master_character_id": character_id, "name": "Purged"
        }).unwrap();

        userdata::delete_account(uid);

        assert!(database::get_song(music_id).is_none(), "the song survived the purge");
        assert!(fs::metadata(&dir).is_err(), "the song's files survived the purge");
        assert!(fs::read(audio_file_path(&play)).is_err(), "the song's audio survived the purge");
        assert!(crate::database::custom_3dmv::get_mv(mv_id).is_none(), "the MV survived the purge");
        assert!(crate::database::custom_card::get_card(card_id).is_none(), "the card survived the purge");
        assert!(crate::database::custom_card::get_character(character_id).is_none(), "the character survived the purge");
    }


    // D1/D6/D7: the whole upload route, end to end - a real multipart body, a real
    // session, the expansion of a real package, and the create running on the
    // blocking pool instead of on the actix worker
    #[test]
    fn the_upload_route_runs_end_to_end_under_its_caps() {
        let _lock = crate::runtime::lock_test_data_path();
        let (uid, cookie) = webui_session("custom-song-upload-route");
        purge_owner(uid);

        let post = |parts: Vec<(&'static str, Vec<u8>)>| -> String {
            let cookie = cookie.clone();
            actix_web::rt::System::new().block_on(async move {
                let payload = multipart_of(parts).await;
                let req = TestRequest::default()
                    .insert_header(("Cookie", format!("ew_token={}", cookie)))
                    .to_http_request();
                let resp = upload(req, payload).await;
                let bytes = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
                String::from_utf8_lossy(&bytes).to_string()
            })
        };

        // A normal upload lands
        let body = post(vec![
            ("name", b"Route Song".to_vec()),
            ("artist", b"A".to_vec()),
            ("attribute", b"1".to_vec()),
            ("jacket", seeded_png(211)),
            ("audio", test_ogg_tone(211.0)),
            ("chart_1", test_chart())
        ]);
        let music_id = jzon::parse(&body).unwrap()["music_id"].as_i64().unwrap_or(0);
        assert!(music_id >= database::FIRST_MUSIC_ID, "{}", body);

        // Its own export package re-imports through the same route
        let package = package::build(music_id).unwrap();
        let body = post(vec![("package", package)]);
        assert!(jzon::parse(&body).unwrap()["music_id"].as_i64().unwrap_or(0) > music_id, "{}", body);

        // An oversized field never reaches the decoders
        let body = post(vec![("audio", vec![b'a'; MAX_FILE_BYTES + 1])]);
        assert!(body.contains("per-file limit"), "{}", body);

        // ...and neither does one that only appears once the package is expanded
        let mut zip = zip::ZipWriter::new(std::io::Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(jzon::stringify(object!{
            "format": 1, "name": "Bomb", "artist": "A", "attribute": 1,
            "levels": [{ "level": 1, "level_number": 3 }]
        }).as_bytes()).unwrap();
        zip.start_file("jacket", options).unwrap();
        zip.write_all(&seeded_png(212)).unwrap();
        zip.start_file("audio", options).unwrap();
        zip.write_all(&vec![0u8; MAX_FILE_BYTES + 1]).unwrap();
        zip.start_file("chart_1.json", options).unwrap();
        zip.write_all(&test_chart()).unwrap();
        let body = post(vec![("package", zip.finish().unwrap().into_inner())]);
        assert!(body.contains("per-file limit"), "{}", body);

        purge_owner(uid);
    }

    // D4/D13: replacing a cue collects the ogg only that song referenced, and
    // leaves one another song still names. The GC runs INSIDE the upload lock, so
    // it cannot observe an upload that has written its oggs but not yet its row
    #[test]
    fn a_replaced_cue_is_collected_and_a_shared_one_is_kept() {
        let _lock = crate::runtime::lock_test_data_path();
        purge_owner(7714);
        purge_owner(7715);

        let shared = create_song(7714, &song_fields("Shared Audio A", 379.0)).unwrap();
        let other = create_song(7715, &song_fields("Shared Audio B", 379.0)).unwrap();
        let play = database::get_song(shared).unwrap()["sound"]["play"]["md5"].to_string();
        assert_eq!(database::get_song(other).unwrap()["sound"]["play"]["md5"].to_string(), play);

        // Replace the first song's audio: its old cue is still the second's
        let mut edit = HashMap::new();
        edit.insert(String::from("audio"), test_ogg_tone(383.0));
        update_song(shared, &edit).unwrap();
        let replaced = database::get_song(shared).unwrap()["sound"]["play"]["md5"].to_string();
        assert_ne!(replaced, play);
        assert!(fs::read(audio_file_path(&play)).is_ok(), "an ogg another song still serves was unlinked");
        assert!(fs::read(audio_file_path(&replaced)).is_ok());

        // Now nothing else references it: replacing it again collects it
        let mut edit = HashMap::new();
        edit.insert(String::from("audio"), test_ogg_tone(389.0));
        update_song(shared, &edit).unwrap();
        assert!(fs::read(audio_file_path(&replaced)).is_err(), "the orphaned ogg was kept");

        purge_owner(7714);
        purge_owner(7715);
    }

}

mod package;
pub mod vmd;

use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, HttpResponse, Responder, http::header::ContentType};
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::fs;
use std::io::{Cursor, Read};
use std::sync::Mutex;

use crate::router::{global, rich_text, userdata, webui, Login, Api};
use crate::router::custom_song;
use crate::database::custom_3dmv as database;
use crate::database::permissions;
use crate::runtime::get_data_path;
use crate::lock_onto_mutex;

// Runtime-uploaded 3D MVs (MMD PMX models + VMD motions) attached to custom
// songs. The client fetches the catalog from /api/custom_3dmv/list at login
// and drives its MMD live director from the blobs; the server stores every
// file content-addressed and serves it verbatim - it validates structure at
// upload time and owns none of the animation semantics.
//
// MVs are owned by their uploader and draft by default: a draft is served to
// its owner's catalog only, publishing puts it in everyone's. Every catalog
// is additionally filtered by referential closure against the SAME user's
// custom-song catalog: an MV whose music_id the song catalog didn't deliver
// is never served (a published MV for someone else's private song stays
// invisible). Filtering is at the CATALOG level; the data GET is
// content-addressed and sessionless, like a CDN.
//
// Storage layout (under --path):
//   custom_3dmv/blobs/{md5}.bin   every model/stage zip, vmd, config - shared
// Metadata lives in custom_3dmv.db as one JSON blob per MV, in the exact
// shape /api/custom_3dmv/list serves.

// Level 1 = custom songs, 2 = the baked SIF1 card band, 3 = runtime custom
// cards, 4 = multi-live, 5 = custom 3D MVs
pub const PROTOCOL_VERSION: u32 = 5;

// Upload limits, enforced while the multipart field is still streaming (the
// 25MB PayloadConfig in lib.rs binds the String/Bytes extractors, not
// Multipart). Motion VMDs run 20-100MB, hence the larger caps than cards
pub const MAX_FILE_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_REQUEST_BYTES: usize = 256 * 1024 * 1024;
pub const MAX_MVS_PER_USER: i64 = 200;

// Per-account storage quota, counted over the stored file sizes the catalog
// quotes. The MV count alone bounded one account at 200 x 256MB = ~51GB, which is
// not a bound at all; 4GiB is roughly forty full MVs of realistic size (a model
// zip plus 20-100MB of motion VMDs) and is the largest of the three features'
// quotas because MVs are the heaviest thing a user can upload
pub const MAX_BYTES_PER_USER: i64 = 4 * 1024 * 1024 * 1024;

// The longest length-prefixed text field a PMX header may declare (model name and
// comment, JP + EN). The prefix is an attacker-supplied i32 the stage walk used to
// skip over by inflating up to 2GB into a sink, four times per entry - CPU with no
// allocation to show for it. Real PMX name/comment fields are tens of bytes
const MAX_PMX_TEXT_BYTES: i32 = 64 * 1024;

// Slots are 1-based, matching the Live3dMemberMst position convention
pub const MAX_MEMBER_COUNT: i64 = 12;

// The in-game stage scenes a config's "stage" may select, verbatim scene
// names. The first entry is the default the client falls back to when no
// config names one (or names one it doesn't recognize) - rejecting unknown
// names at upload instead gives the author feedback while the typo is fixable
pub const STAGES: &[&str] = &[
    "bg0007_02_s1", "bg0008_01_s1", "bg0014_01_s1", "bg0037_01_s1", "bg0018_02_s1",
    "bg0005_01_s1", "bg0007_03_s1", "bg0003_01_s1", "bg0004_01_s1", "bg0018_01_s1",
    "bg0001_01_s1", "bg0015_01_s1", "bg0027_02_s1", "bg0011_01_s1", "bg0023_01_s1",
    "bg0031_01_s1", "bg0007_01_s1", "bg0017_01_s1", "bg0013_01_s1", "bg0019_01_s1",
    "bg0016_01_s1", "bg0009_01_s1", "bg0007_04_s1", "bg0002_01_s1", "bg0027_01_s1",
    "bg0022_01_s1", "bg0020_01_s1", "bg0032_01_s1", "bg0010_01_s1", "bg0028_01_s1",
    "bg0018_03_s1", "bg0026_02_s1", "bg0029_01_s1", "bg0020_02_s1", "bg0026_01_s1",
    "bg0024_01_s1", "bg0036_01_s1", "bg0012_01_s1", "bg0025_07_s1", "bg0025_11_s1",
    "bg0025_01_s1", "bg0025_06_s1", "bg0006_01_s1", "bg0006_02_s1", "bg0025_02_s1",
    "bg0025_03_s1", "bg0021_01_s1", "bg0025_10_s1", "bg0030_02_s1", "bg0006_03_s1",
    "bg0025_09_s1", "bg0006_04_s1", "bg0007_10_s1", "bg0008_10_s1", "bg0006_10_s1",
    "bg0025_08_s1", "bg0030_01_s1", "bg0025_04_s1", "bg0025_05_s1", "bg0033_01_s1",
    "bg0025_12_s1", "bg0019_02_s1", "bg0038_01_s1"
];

type Fields = HashMap<String, Vec<u8>>;

lazy_static! {
    // Id allocation, blob writes and the insert must not race between two
    // uploads (and the delete-side GC must not race an insert)
    static ref UPLOAD_LOCK: Mutex<()> = Mutex::new(());
}

// Game endpoints (/api scope, standard envelope)
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/custom_3dmv")
            .route("/list", web::post().to(list))
    );
}

// Plain blob GET for the game + session-authenticated management API for the
// webui. Mounted OUTSIDE /api so the game middlewares never wrap it
pub fn web_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/custom_3dmv")
            .route("/data/{hash}/{file}", web::get().to(data))
            .route("/upload", web::post().to(upload))
            .route("/update", web::post().to(update))
            .route("/publish", web::post().to(publish))
            .route("/delete", web::post().to(delete))
            .route("/mine", web::get().to(mine))
            .route("/browse", web::get().to(browse))
            .route("/download/{mv_id}", web::get().to(download))
    );
}

// The whole feature is opt-in (--enable-custom-3dmv) and additionally off in
// --hidden mode. When disabled every endpoint 404s / errors as if it never
// existed and nothing touches custom_3dmv.db (so no table setup runs)
pub fn disabled() -> bool {
    let args = crate::get_args();
    args.hidden || !args.enable_custom_3dmv
}

pub fn blob_path(md5: &str) -> String {
    get_data_path(&format!("custom_3dmv/blobs/{}.bin", md5))
}

// The multipart field name a stored files[] entry came from, which is also
// its name inside an export package
pub fn field_key(file: &JsonValue) -> Option<String> {
    let role = file["role"].as_str()?;
    match file["slot"].as_i64() {
        Some(slot) => Some(format!("{}_{}", role, slot)),
        None => Some(role.to_string())
    }
}

// The music ids this user's song catalog delivers - the closure set every MV
// catalog is filtered against. Empty when custom songs are disabled, which
// correctly serves no MVs at all: there is nothing they could play over
fn allowed_music_ids(uid: i64) -> Vec<i64> {
    custom_song::get_music_ids(uid).members().filter_map(|id| id.as_i64()).collect()
}

pub fn catalog_for_user(uid: i64) -> JsonValue {
    database::get_mvs_for_user(uid, &allowed_music_ids(uid))
}

// The catalog is filtered per requesting user: everyone gets the published
// MVs, the owner additionally gets their drafts, both closed over the same
// user's song catalog. Old clients get Api(None), feature-off semantics
async fn list(req: HttpRequest, Login(key): Login) -> impl Responder {
    if disabled() {
        // As if the endpoint doesn't exist - the client treats this as feature-off
        return Api(None);
    }
    if global::client_protocol_version(&req) < PROTOCOL_VERSION {
        return Api(None);
    }
    let uid = userdata::get_acc(&key)["user"]["id"].as_i64().unwrap();
    Api(Some(object!{
        "revision": database::get_revision(),
        "mvs": catalog_for_user(uid)
    }))
}

// Content-addressed blob fetch: '{server}/custom_3dmv/data/{md5}/{md5}.bin'.
// The game builds the URL from the md5 it read in the catalog and caches by
// it, so a stale md5 simply 404s and the client re-downloads under the new
// one. Visible to all like the other custom data routes (CDN semantics) -
// only the feature flag gates it
async fn data(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let hash = req.match_info().get("hash").unwrap_or("").to_string();
    let file = req.match_info().get("file").unwrap_or("").to_string();
    if hash.len() != 32 || !hash.chars().all(|c| c.is_ascii_hexdigit()) || !file.starts_with(&format!("{}.", hash)) {
        return HttpResponse::NotFound().finish();
    }
    if !database::find_blob_by_md5(&hash) {
        return HttpResponse::NotFound().finish();
    }
    match fs::read(blob_path(&hash)) {
        Ok(body) => {
            HttpResponse::Ok()
                .insert_header(ContentType::octet_stream())
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

// The per-file cap is enforced while the field is still streaming, BEFORE any
// byte reaches the zip/vmd parsers. The per-request cap is checked over the
// running total
async fn read_multipart(mut payload: Multipart) -> Result<Fields, String> {
    let mut fields = Fields::new();
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
fn check_field_caps(fields: &Fields) -> Result<(), String> {
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

fn field_str(fields: &Fields, key: &str) -> String {
    String::from_utf8_lossy(fields.get(key).map(|v| v.as_slice()).unwrap_or(&[])).trim().to_string()
}

// Checkbox-style flag: "1", "true" or "on"
fn field_flag(fields: &Fields, key: &str) -> bool {
    matches!(field_str(fields, key).to_lowercase().as_str(), "1" | "true" | "on")
}

fn file_of<'a>(fields: &'a Fields, key: &str) -> Option<&'a Vec<u8>> {
    fields.get(key).filter(|v| !v.is_empty())
}

// Partial-edit semantics for update: a field present in the form replaces the
// stored value, an absent one keeps it. On create `stored` is empty, so every
// absent field simply reads as empty/invalid and fails its own validation
fn text_of(fields: &Fields, key: &str, stored: &JsonValue, stored_key: &str) -> String {
    if fields.contains_key(key) {
        field_str(fields, key)
    } else {
        stored[stored_key].as_str().unwrap_or("").to_string()
    }
}

fn number_of(fields: &Fields, key: &str, stored: &JsonValue, stored_key: &str) -> i64 {
    if fields.contains_key(key) {
        field_str(fields, key).parse::<i64>().unwrap_or(i64::MIN)
    } else {
        stored[stored_key].as_i64().unwrap_or(i64::MIN)
    }
}

// The PMX header walk from just past the version to the vertex count: the
// count-prefixed globals block, four length-prefixed text fields (model name
// and comment, JP + EN), then the i32 vertex count. None on any truncation
// or nonsense length
fn read_pmx_vertex_count(file: &mut impl Read) -> Option<i32> {
    let mut count = [0u8; 1];
    file.read_exact(&mut count).ok()?;
    let mut globals = vec![0u8; count[0] as usize];
    file.read_exact(&mut globals).ok()?;
    for _ in 0..4 {
        let mut len = [0u8; 4];
        file.read_exact(&mut len).ok()?;
        let len = i32::from_le_bytes(len);
        if !(0..=MAX_PMX_TEXT_BYTES).contains(&len) {
            return None;
        }
        if std::io::copy(&mut file.by_ref().take(len as u64), &mut std::io::sink()).ok()? != len as u64 {
            return None;
        }
    }
    let mut vertices = [0u8; 4];
    file.read_exact(&mut vertices).ok()?;
    Some(i32::from_le_bytes(vertices))
}

// A model upload is a zip carrying at least one .pmx entry. Only the header
// is read from each entry's stream: 4-byte magic "PMX " and the f32 version,
// which must be 2.0 or 2.1 (the versions the client's parser speaks). For a
// custom stage the walk continues to the vertex count - a zero-vertex stage
// renders as nothing, so it is rejected while the author can still fix it
fn validate_model_zip(label: &str, bytes: &[u8], require_vertices: bool) -> Result<(), String> {
    let mut archive = zip::ZipArchive::new(Cursor::new(bytes))
        .map_err(|_| format!("'{}' is not a valid zip file", label))?;
    let mut found = false;
    for i in 0..archive.len() {
        let mut file = archive.by_index(i).map_err(|e| format!("'{}': {}", label, e))?;
        let name = file.name().to_string();
        if !name.to_lowercase().ends_with(".pmx") {
            continue;
        }
        let mut header = [0u8; 8];
        file.read_exact(&mut header)
            .map_err(|_| format!("'{}': entry '{}' is too short to be a PMX model", label, name))?;
        if &header[..4] != b"PMX " {
            return Err(format!("'{}': entry '{}' is missing the \"PMX \" magic", label, name));
        }
        let version = f32::from_le_bytes(header[4..8].try_into().unwrap());
        if (version - 2.0).abs() > 0.001 && (version - 2.1).abs() > 0.001 {
            return Err(format!("'{}': entry '{}' is PMX version {} - only 2.0 and 2.1 are supported", label, name, version));
        }
        if require_vertices {
            let vertices = read_pmx_vertex_count(&mut file)
                .ok_or(format!("'{}': entry '{}' has a malformed PMX header", label, name))?;
            if vertices <= 0 {
                return Err(format!("'{}': entry '{}' has no vertices - a stage model needs geometry", label, name));
            }
        }
        found = true;
    }
    if !found {
        return Err(format!("'{}' contains no .pmx model entry", label));
    }
    Ok(())
}

// The config is otherwise opaque driver knobs with a client-defined schema,
// but "stage" is worth validating server-side: the client silently falls back
// to the default stage for a name it doesn't know
fn validate_config(label: &str, bytes: &[u8]) -> Result<(), String> {
    let config = jzon::parse(&String::from_utf8_lossy(bytes)).map_err(|_| format!("'{}' is not valid JSON", label))?;
    if !config["stage"].is_null() {
        match config["stage"].as_str() {
            Some(stage) if STAGES.contains(&stage) => {},
            _ => return Err(format!("'{}': unknown stage '{}' - it must be one of the in-game stage scene names the limits endpoint lists", label, config["stage"]))
        }
    }
    // The custom-stage world scale the client applies (its default is 0.08,
    // clamped to this same range)
    if !config["stage_scale"].is_null() {
        match config["stage_scale"].as_f64() {
            Some(scale) if (0.005..=1.0).contains(&scale) => {},
            _ => return Err(format!("'{}': stage_scale must be a number between 0.005 and 1, not {}", label, config["stage_scale"]))
        }
    }
    Ok(())
}

fn validate_file(role: &str, label: &str, bytes: &[u8]) -> Result<(), String> {
    match role {
        "model" => validate_model_zip(label, bytes, false),
        // A custom stage overrides the config's "stage" scene client-side
        "stage" => validate_model_zip(label, bytes, true),
        "config" => validate_config(label, bytes),
        _ => vmd::validate(label, bytes)
    }
}

struct PendingBlob {
    md5: String,
    bytes: Vec<u8>
}

// One (role, slot) resolved against the form and the stored files: a new file
// replaces (validated first), a `{key}_delete` flag drops an optional role,
// an absent field keeps the stored entry. The entry's md5 is the hash of the
// exact bytes the data route serves
fn resolve_file(
    fields: &Fields, stored: &JsonValue, role: &str, slot: Option<i64>, required: bool,
    pending: &mut Vec<PendingBlob>
) -> Result<Option<JsonValue>, String> {
    let key = match slot {
        Some(slot) => format!("{}_{}", role, slot),
        None => role.to_string()
    };
    let file = file_of(fields, &key);
    if field_flag(fields, &format!("{}_delete", key)) {
        if file.is_some() {
            return Err(format!("'{}': cannot both replace and delete the same file", key));
        }
        if required {
            return Err(format!("'{}' cannot be deleted - every member slot needs a model and a motion", key));
        }
        return Ok(None);
    }
    if let Some(bytes) = file {
        validate_file(role, &key, bytes)?;
        let md5 = format!("{:x}", md5::compute(bytes));
        let mut entry = object!{ "role": role };
        if let Some(slot) = slot {
            entry["slot"] = slot.into();
        }
        entry["md5"] = md5.clone().into();
        entry["size"] = bytes.len().into();
        pending.push(PendingBlob { md5, bytes: bytes.clone() });
        return Ok(Some(entry));
    }
    let kept = stored.members().find(|f| f["role"] == role && slot.map_or(true, |slot| f["slot"] == slot));
    if let Some(kept) = kept {
        return Ok(Some(kept.clone()));
    }
    if required {
        return Err(format!("'{}' is required - every member slot needs a model and a motion", key));
    }
    Ok(None)
}

// The resulting files array for `member_count` slots. Every slot 1..count
// must end up with a model and a motion; facial (per slot) and the slot-less
// camera, config and stage are optional. A member_count decrease simply stops
// visiting the higher slots, whose stored entries drop out (and their blobs GC)
fn collect_files(fields: &Fields, member_count: i64, stored: &JsonValue) -> Result<(JsonValue, Vec<PendingBlob>), String> {
    let mut entries = array![];
    let mut pending = Vec::new();
    for slot in 1..=member_count {
        for (role, required) in [("model", true), ("motion", true), ("facial", false)] {
            if let Some(entry) = resolve_file(fields, stored, role, Some(slot), required, &mut pending)? {
                entries.push(entry).unwrap();
            }
        }
    }
    for role in ["camera", "config", "stage"] {
        if let Some(entry) = resolve_file(fields, stored, role, None, false, &mut pending)? {
            entries.push(entry).unwrap();
        }
    }
    Ok((entries, pending))
}

fn write_blobs(pending: &[PendingBlob]) -> Result<(), String> {
    if pending.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(get_data_path("custom_3dmv/blobs")).map_err(|e| e.to_string())?;
    for blob in pending {
        fs::write(blob_path(&blob.md5), &blob.bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Blobs are content-addressed and may be shared between MVs and roles, so a
// file is only unlinked when no live row references its md5 anymore. Called
// under UPLOAD_LOCK after the db row changed, so the row's own surviving
// references still protect their blobs
fn gc_blobs(old_files: &JsonValue) {
    for file in old_files.members() {
        let md5 = file["md5"].to_string();
        // A read error means "assume referenced": unlinking on a doubtful
        // reference set is exactly what the startup sweep refuses to do
        if md5.len() == 32 && !database::blob_in_use(&md5).unwrap_or(true) {
            let _ = fs::remove_file(blob_path(&md5));
        }
    }
}

// Like custom songs, MVs are permissionless beyond login: any logged-in user
// manages (and publishes) their own. 3dmv.edit is moderation over anybody's
fn can_manage(uid: i64, owner: i64) -> bool {
    owner == uid || permissions::has(uid, permissions::MV_EDIT)
}

fn validate_names(name: &str, name_en: &str) -> Result<(), String> {
    if name.is_empty() {
        return Err(String::from("MV name is required"));
    }
    // Rendered through TMP with rich text on and no escaping, like every
    // other custom-content name (rich_text.rs)
    rich_text::reject_tags("MV name", name, &[])?;
    rich_text::reject_tags("MV English name", name_en, &[])
}

pub fn create_mv(uid: i64, fields: &Fields) -> Result<i64, String> {
    if database::mv_count_for_owner(uid) >= MAX_MVS_PER_USER {
        return Err(format!("You have reached the {} MV limit", MAX_MVS_PER_USER));
    }
    let published = field_flag(fields, "published");

    let name = field_str(fields, "name");
    let name_en = field_str(fields, "name_en");
    validate_names(&name, &name_en)?;

    let music_id = field_str(fields, "music_id").parse::<i64>().unwrap_or(0);
    custom_song::can_reference_song(uid, music_id)?;

    let member_count = field_str(fields, "member_count").parse::<i64>().unwrap_or(0);
    if !(1..=MAX_MEMBER_COUNT).contains(&member_count) {
        return Err(format!("member_count must be 1-{}", MAX_MEMBER_COUNT));
    }

    let (files, pending) = collect_files(fields, member_count, &array![])?;
    check_quota(uid, database::mv_bytes(&files), 0)?;

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let mv_id = database::next_mv_id();
    if mv_id > database::LAST_MV_ID {
        return Err(String::from("The custom MV id space is exhausted"));
    }

    let mv = object!{
        "mv_id": mv_id,
        "music_id": music_id,
        "name": name,
        "name_en": name_en,
        "member_count": member_count,
        "files": files
    };

    write_blobs(&pending)?;
    database::insert_mv(mv_id, music_id, uid, &mv, published)
        .map_err(|e| format!("Could not store the MV: {}", e))?;
    database::bump_revision();
    drop(lock);

    Ok(mv_id)
}

// Per-account storage quota. `excluded` is the MV being replaced by an in-place
// edit - its stored size drops out and `adding` (the resulting size) replaces it
fn check_quota(uid: i64, adding: i64, excluded_mv_id: i64) -> Result<(), String> {
    let used = database::owner_bytes(uid, excluded_mv_id);
    if used + adding > MAX_BYTES_PER_USER {
        return Err(format!(
            "This upload would put your MVs at {} MB, over the {} MB per-account limit - delete an MV first",
            (used + adding) / (1024 * 1024), MAX_BYTES_PER_USER / (1024 * 1024)
        ));
    }
    Ok(())
}

// Edit an MV in place. The mv_id and the music_id stay the same: repointing
// the song would break the catalog closure for everyone who already resolved
// it (delete + re-upload retires the id instead)
pub fn update_mv(uid: i64, mv_id: i64, fields: &Fields) -> Result<(), String> {
    let Some(owner) = database::get_mv_owner(mv_id) else {
        return Err(String::from("MV not found"));
    };
    if !can_manage(uid, owner) {
        return Err(String::from("You can only manage your own MVs"));
    }
    let stored = database::get_mv(mv_id).ok_or(String::from("MV not found"))?;

    let name = text_of(fields, "name", &stored, "name");
    let name_en = text_of(fields, "name_en", &stored, "name_en");
    validate_names(&name, &name_en)?;

    let member_count = number_of(fields, "member_count", &stored, "member_count");
    if !(1..=MAX_MEMBER_COUNT).contains(&member_count) {
        return Err(format!("member_count must be 1-{}", MAX_MEMBER_COUNT));
    }

    let (files, pending) = collect_files(fields, member_count, &stored["files"])?;
    check_quota(owner, database::mv_bytes(&files), mv_id)?;

    let mv = object!{
        "mv_id": mv_id,
        "music_id": stored["music_id"].clone(),
        "name": name,
        "name_en": name_en,
        "member_count": member_count,
        "files": files
    };

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    write_blobs(&pending)?;
    database::update_mv(mv_id, &mv);
    database::bump_revision();
    // Replaced/dropped files: the updated row no longer references them
    gc_blobs(&stored["files"]);
    drop(lock);

    Ok(())
}

pub fn set_mv_flags(uid: i64, mv_id: i64, published: bool) -> Result<(), String> {
    let Some(owner) = database::get_mv_owner(mv_id) else {
        return Err(String::from("MV not found"));
    };
    if !can_manage(uid, owner) {
        return Err(String::from("You can only manage your own MVs"));
    }
    database::set_published(mv_id, published);
    database::bump_revision();
    Ok(())
}

// Deleting retires the id forever (the high-water mark never reissues it)
pub fn delete_mv(uid: i64, mv_id: i64) -> Result<(), String> {
    let Some(owner) = database::get_mv_owner(mv_id) else {
        return Err(String::from("MV not found"));
    };
    if !can_manage(uid, owner) {
        return Err(String::from("You can only manage your own MVs"));
    }
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let stored = database::get_mv(mv_id);
    database::delete_mv(mv_id);
    database::bump_revision();
    if let Some(stored) = stored {
        gc_blobs(&stored["files"]);
    }
    drop(lock);
    Ok(())
}

// The delete cascade: an MV can't outlive the song it plays over. Called from
// custom_song's delete handler (which holds ITS upload lock - a different
// mutex, and nothing ever takes the two in the reverse order)
pub fn purge_song(music_id: i64) {
    if disabled() {
        return;
    }
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let mut purged = false;
    for mv_id in database::mv_ids_for_music(music_id) {
        let stored = database::get_mv(mv_id);
        database::delete_mv(mv_id);
        purged = true;
        if let Some(stored) = stored {
            gc_blobs(&stored["files"]);
        }
    }
    if purged {
        database::bump_revision();
    }
    drop(lock);
}

// Every MV this account uploaded, gone - called from userdata::delete_account, so
// a purged uploader leaves no catalog row resolving an owner id that no longer
// exists (browse renders an uploader name for every row). Same steps as the
// owner's own delete, blob GC included
pub fn purge_owner(uid: i64) {
    if disabled() {
        return;
    }
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let mut purged = false;
    for mv_id in database::mv_ids_for_owner(uid) {
        let stored = database::get_mv(mv_id);
        database::delete_mv(mv_id);
        purged = true;
        if let Some(stored) = stored {
            gc_blobs(&stored["files"]);
        }
    }
    if purged {
        database::bump_revision();
    }
    drop(lock);
}

// Startup GC for the content-addressed blob store, mirroring
// custom_song::sweep_audio: the only writers are upload and update, so an
// unreferenced file is a leftover from an interrupted one. Deliberately
// fail-closed - anything that makes the reference set doubtful (unreadable
// catalog, unparseable blob, an entry without a proper md5) aborts the whole
// sweep instead of treating that MV as referencing nothing. Only exactly
// {32 hex}.bin names are ever considered
pub fn sweep_blobs() {
    if disabled() {
        return;
    }
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let Some(blobs) = database::all_mv_blobs() else {
        println!("Custom 3DMV blob sweep: catalog unreadable, skipped");
        return;
    };
    let mut referenced: Vec<String> = Vec::new();
    for blob in blobs.members() {
        let Ok(mv) = jzon::parse(&blob.to_string()) else {
            println!("Custom 3DMV blob sweep: unparseable catalog row, skipped");
            return;
        };
        if mv["files"].is_empty() {
            println!("Custom 3DMV blob sweep: MV {} has no files, skipped", mv["mv_id"]);
            return;
        }
        for file in mv["files"].members() {
            let md5 = file["md5"].as_str().unwrap_or("");
            if md5.len() != 32 {
                println!("Custom 3DMV blob sweep: MV {} has a malformed file entry, skipped", mv["mv_id"]);
                return;
            }
            referenced.push(String::from(md5));
        }
    }

    // No directory means nothing was ever uploaded
    let Ok(entries) = fs::read_dir(get_data_path("custom_3dmv/blobs")) else {
        return;
    };
    let mut removed = 0;
    for entry in entries.flatten() {
        let name = entry.file_name().to_string_lossy().to_string();
        let Some(md5) = name.strip_suffix(".bin") else { continue; };
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
        println!("Custom 3DMV blob sweep: removed {} orphaned blob(s)", removed);
    }
    drop(lock);
}

// The concrete upload bounds, served to the webui so the form can enforce
// them client-side
pub fn upload_limits() -> JsonValue {
    object!{
        "max_member_count": MAX_MEMBER_COUNT,
        "max_file_bytes": MAX_FILE_BYTES,
        "max_request_bytes": MAX_REQUEST_BYTES,
        "max_mvs_per_user": MAX_MVS_PER_USER,
        "max_bytes_per_user": MAX_BYTES_PER_USER,
        "stages": STAGES.to_vec(),
        "default_stage": STAGES[0],
        "roles": {
            "model":  { "per_slot": true,  "required": true,  "kind": "pmx-zip" },
            "motion": { "per_slot": true,  "required": true,  "kind": "vmd" },
            "facial": { "per_slot": true,  "required": false, "kind": "vmd" },
            "camera": { "per_slot": false, "required": false, "kind": "vmd" },
            "config": { "per_slot": false, "required": false, "kind": "json" },
            "stage":  { "per_slot": false, "required": false, "kind": "pmx-zip" }
        }
    }
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
    // Zip inflation, the PMX/VMD structure walks, hashing and writing up to 256MB:
    // all of it on the blocking pool rather than on the actix worker that also has
    // to keep serving the game API
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
        create_mv(uid, &fields)
    }).await;
    match result {
        Ok(Ok(mv_id)) => send_json(object!{
            result: "OK",
            mv_id: mv_id
        }),
        Ok(Err(e)) => webui::error(&e),
        Err(_) => webui::error("The upload could not be processed")
    }
}

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
    let mv_id = field_str(&fields, "mv_id").parse::<i64>().unwrap_or(0);
    match web::block(move || update_mv(uid, mv_id, &fields)).await {
        Ok(Ok(())) => send_json(object!{
            result: "OK",
            mv_id: mv_id
        }),
        Ok(Err(e)) => webui::error(&e),
        Err(_) => webui::error("The edit could not be processed")
    }
}

async fn publish(req: HttpRequest, body: String) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    let body = jzon::parse(&body).unwrap_or(object!{});
    let Some(published) = body["published"].as_bool() else {
        return webui::error("published must be true or false");
    };
    match set_mv_flags(uid, body["mv_id"].as_i64().unwrap_or(0), published) {
        Ok(()) => send_json(object!{
            result: "OK"
        }),
        Err(e) => webui::error(&e)
    }
}

async fn delete(req: HttpRequest, body: String) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    let body = jzon::parse(&body).unwrap_or(object!{});
    match delete_mv(uid, body["mv_id"].as_i64().unwrap_or(0)) {
        Ok(()) => send_json(object!{
            result: "OK"
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
        mvs: database::get_mvs_by_owner(uid)
    })
}

// The public MV browser: the published catalog closed over the songs the
// viewer can see, with uploader names. Anonymous viewers get the MVs on
// public songs - published means public
async fn browse(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let viewer = get_session_uid(&req).unwrap_or(0);
    let mut mvs = database::get_browse_mvs(&allowed_music_ids(viewer));
    for mv in mvs.members_mut() {
        mv["uploader"] = userdata::get_name_and_rank(mv["owner_id"].as_i64().unwrap_or(0))["user_name"].clone();
        mv.remove("owner_id");
    }
    send_json(object!{
        result: "OK",
        mvs: mvs
    })
}

// Download an MV as an export package, re-uploadable on any ew server. The
// viewer must be able to see it: their own, or published
async fn download(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let mv_id = req.match_info().get("mv_id").unwrap_or("").parse::<i64>().unwrap_or(0);
    let Some(owner) = database::get_mv_owner(mv_id) else {
        return webui::error("MV not found");
    };
    let viewer = get_session_uid(&req);
    if viewer != Some(owner) && !database::is_published(mv_id) {
        return webui::error("MV not found");
    }
    // Every catalog (list, browse, mine) additionally closes over the songs the
    // viewer can see, so a published MV attached to someone else's PRIVATE song is
    // invisible everywhere - it must not be downloadable by walking mv_ids either
    let music_id = database::get_mv_music_id(mv_id).unwrap_or(0);
    if viewer != Some(owner) && !allowed_music_ids(viewer.unwrap_or(0)).contains(&music_id) {
        return webui::error("MV not found");
    }
    match package::build(mv_id) {
        Ok(bytes) => {
            HttpResponse::Ok()
                .insert_header(("content-type", "application/zip"))
                .insert_header(("content-disposition", format!("attachment; filename=\"custom_3dmv_{}.zip\"", mv_id)))
                .insert_header(("content-length", bytes.len()))
                .body(bytes)
        },
        Err(e) => webui::error(&e)
    }
}

#[cfg(test)]
pub mod tests {
    use super::*;
    use std::io::Write;
    use crate::router::custom_card::tests::with_permissions;

    pub fn field(fields: &mut Fields, key: &str, value: &str) {
        fields.insert(String::from(key), value.as_bytes().to_vec());
    }

    // A structurally complete VMD: 2 bone keys, 1 morph, 1 camera, empty
    // light/shadow sections and 1 property key with 1 IK toggle. The seed
    // lands in the model-name padding and the record tails, so different
    // seeds give different md5s
    pub fn test_vmd(seed: u8) -> Vec<u8> {
        let mut rv = Vec::new();
        rv.extend(b"Vocaloid Motion Data 0002");
        rv.resize(30, 0);
        rv.extend(b"TestModel");
        rv.resize(50, 0);
        rv[49] = seed;
        rv.extend(2u32.to_le_bytes());
        for i in 0..2u8 {
            let mut record = vec![0u8; 111];
            record[0] = b'b';
            record[1] = b'0' + i;
            record[15..19].copy_from_slice(&(i as u32 * 30).to_le_bytes());
            record[110] = seed;
            rv.extend(record);
        }
        rv.extend(1u32.to_le_bytes());
        let mut morph = vec![0u8; 23];
        morph[0] = b'm';
        morph[22] = seed;
        rv.extend(morph);
        rv.extend(1u32.to_le_bytes());
        let mut camera = vec![0u8; 61];
        camera[60] = seed;
        rv.extend(camera);
        rv.extend(0u32.to_le_bytes());
        rv.extend(0u32.to_le_bytes());
        rv.extend(1u32.to_le_bytes());
        rv.extend(0u32.to_le_bytes());
        rv.push(1);
        rv.extend(1u32.to_le_bytes());
        let mut ik = vec![0u8; 21];
        ik[0] = b'i';
        ik[20] = 1;
        rv.extend(ik);
        rv
    }

    // A camera-only VMD: empty bone/morph sections, 2 camera keys, then EOF -
    // the later sections are legitimately absent
    pub fn test_camera_vmd(seed: u8) -> Vec<u8> {
        let mut rv = Vec::new();
        rv.extend(b"Vocaloid Motion Data 0002");
        rv.resize(30, 0);
        rv.extend(b"CameraModel");
        rv.resize(50, 0);
        rv[49] = seed;
        rv.extend(0u32.to_le_bytes());
        rv.extend(0u32.to_le_bytes());
        rv.extend(2u32.to_le_bytes());
        for i in 0..2u8 {
            let mut record = vec![0u8; 61];
            record[..4].copy_from_slice(&(i as u32 * 30).to_le_bytes());
            record[60] = seed;
            rv.extend(record);
        }
        rv
    }

    fn zip_with(name: &str, bytes: &[u8]) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        zip.start_file(name, zip::write::SimpleFileOptions::default()).unwrap();
        zip.write_all(bytes).unwrap();
        zip.finish().unwrap().into_inner()
    }

    // A minimal model zip: one PMX entry (magic + version + a seeded tail)
    // plus a texture entry the validator must skip over
    pub fn test_pmx_zip(seed: u8, version: f32) -> Vec<u8> {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("model.pmx", options).unwrap();
        let mut pmx = Vec::new();
        pmx.extend(b"PMX ");
        pmx.extend(version.to_le_bytes());
        pmx.extend([8u8, seed, 0, 0]);
        zip.write_all(&pmx).unwrap();
        zip.start_file("tex/body.png", options).unwrap();
        zip.write_all(&[seed, 1, 2, 3]).unwrap();
        zip.finish().unwrap().into_inner()
    }

    // A stage PMX zip with the full header walk to the vertex count: globals,
    // four empty text fields, then `vertices`. The seed lands in the globals
    // so different seeds give different md5s
    pub fn test_stage_zip(seed: u8, vertices: i32) -> Vec<u8> {
        let mut pmx = Vec::new();
        pmx.extend(b"PMX ");
        pmx.extend(2.0f32.to_le_bytes());
        pmx.push(8);
        pmx.extend([0, 0, 0, 0, 0, 0, 0, seed]);
        for _ in 0..4 {
            pmx.extend(0u32.to_le_bytes());
        }
        pmx.extend(vertices.to_le_bytes());
        zip_with("stage.pmx", &pmx)
    }

    // A catalog row is all an MV needs from a song; the full upload pipeline
    // is custom_song's own test surface
    pub fn seed_song(music_id: i64, owner: i64, visibility: &str) {
        crate::database::custom_song::insert_song(music_id, owner, &object!{
            "music_id": music_id,
            "name": format!("Seed Song {}", music_id),
            "sound": { "play": { "md5": "0".repeat(32) }, "select": { "md5": "0".repeat(32) } }
        }, visibility, &array![], false).unwrap();
    }

    // A complete, valid 2-slot upload: model+motion per slot, a facial on
    // slot 1, a camera and a config. Seeds must be >= 5 apart between tests
    // (slot files use seed+slot, the facial seed+3)
    pub fn base_fields(music_id: i64, member_count: i64, seed: u8) -> Fields {
        let mut fields = Fields::new();
        field(&mut fields, "name", "Test MV");
        field(&mut fields, "name_en", "Test MV EN");
        field(&mut fields, "music_id", &music_id.to_string());
        field(&mut fields, "member_count", &member_count.to_string());
        for slot in 1..=member_count {
            fields.insert(format!("model_{}", slot), test_pmx_zip(seed + slot as u8, 2.0));
            fields.insert(format!("motion_{}", slot), test_vmd(seed + slot as u8));
        }
        fields.insert(String::from("facial_1"), test_vmd(seed + 3));
        fields.insert(String::from("camera"), test_camera_vmd(seed));
        fields.insert(String::from("config"), br#"{"scale":1.0,"world_offset":[0,0,0]}"#.to_vec());
        fields
    }

    pub fn wipe(uid: i64) {
        for mv in database::get_mvs_by_owner(uid).members() {
            let _ = delete_mv(uid, mv["mv_id"].as_i64().unwrap());
        }
    }

    fn file_md5(mv: &JsonValue, role: &str, slot: Option<i64>) -> String {
        mv["files"].members()
            .find(|f| f["role"] == role && slot.map_or(f["slot"].is_null(), |slot| f["slot"] == slot))
            .map(|f| f["md5"].to_string()).unwrap_or_default()
    }

    // The whole feature is off unless --enable-custom-3dmv: endpoints 404 and
    // the cross-module helpers (the song-delete cascade) never touch the table
    #[test]
    fn feature_gate_hides_everything_when_disabled() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_020);
        seed_song(970099, 9_100_020, "public");
        let id = create_mv(9_100_020, &base_fields(970099, 1, 90)).unwrap();

        crate::runtime::set_enable_custom_3dmv(false);
        assert!(disabled());
        let resp = actix_web::rt::System::new().block_on(async {
            data(actix_web::test::TestRequest::default().to_http_request()).await
        });
        assert_eq!(resp.status(), actix_web::http::StatusCode::NOT_FOUND);
        // The cascade is a no-op while disabled - nothing may touch the db
        purge_song(970099);
        crate::runtime::set_enable_custom_3dmv(true);
        assert!(database::get_mv(id).is_some(), "disabled purge must not touch the table");

        // Enabled again, the same cascade works
        purge_song(970099);
        assert!(database::get_mv(id).is_none());
        wipe(9_100_020);
    }

    // A full create: the catalog entry the client parses, the blob store, the
    // md5 index, draft visibility and the publish flip
    #[test]
    fn upload_happy_path_builds_the_catalog_entry() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_001);
        wipe(9_100_002);
        seed_song(970001, 9_100_001, "public");

        let fields = base_fields(970001, 2, 10);
        let id = create_mv(9_100_001, &fields).unwrap();
        assert!(id >= database::FIRST_MV_ID);

        let mv = database::get_mv(id).unwrap();
        assert_eq!(mv["mv_id"].as_i64(), Some(id));
        assert_eq!(mv["music_id"].as_i64(), Some(970001));
        assert_eq!(mv["name"].as_str(), Some("Test MV"));
        assert_eq!(mv["name_en"].as_str(), Some("Test MV EN"));
        assert_eq!(mv["member_count"].as_i64(), Some(2));
        // model+motion per slot, facial on slot 1, camera, config
        assert_eq!(mv["files"].len(), 7);
        for slot in 1..=2 {
            for role in ["model", "motion"] {
                assert!(mv["files"].members().any(|f| f["role"] == role && f["slot"] == slot), "{} {}", role, slot);
            }
        }
        assert!(mv["files"].members().any(|f| f["role"] == "facial" && f["slot"] == 1));
        assert!(mv["files"].members().any(|f| f["role"] == "camera" && f["slot"].is_null()));
        assert!(mv["files"].members().any(|f| f["role"] == "config" && f["slot"].is_null()));

        // Every entry hashes the exact bytes in the blob store and the data
        // route's index resolves it
        for file in mv["files"].members() {
            let md5 = file["md5"].to_string();
            assert_eq!(md5.len(), 32);
            let bytes = fs::read(blob_path(&md5)).unwrap();
            assert_eq!(format!("{:x}", md5::compute(&bytes)), md5);
            assert_eq!(bytes.len(), file["size"].as_usize().unwrap());
            assert!(database::find_blob_by_md5(&md5));
        }
        assert!(!database::find_blob_by_md5(&"f".repeat(32)));

        // A draft: owner-only
        assert!(catalog_for_user(9_100_001).members().any(|m| m["mv_id"] == id));
        assert!(!catalog_for_user(9_100_002).members().any(|m| m["mv_id"] == id));

        // The owner publishes without any scope; once published (and the song
        // is public) everyone resolves it
        set_mv_flags(9_100_001, id, true).unwrap();
        assert!(database::is_published(id));
        assert!(catalog_for_user(9_100_002).members().any(|m| m["mv_id"] == id));

        // The export package round-trips through expand into the same fields
        let package = package::build(id).unwrap();
        let mut expanded = Fields::new();
        package::expand(&package, &mut expanded).unwrap();
        assert_eq!(field_str(&expanded, "name"), "Test MV");
        assert_eq!(field_str(&expanded, "music_id"), "970001");
        assert_eq!(field_str(&expanded, "member_count"), "2");
        for key in ["model_1", "motion_1", "facial_1", "model_2", "motion_2", "camera", "config"] {
            assert_eq!(expanded.get(key), fields.get(key), "package entry {}", key);
        }
        // A form-supplied music_id survives the expand (server-local id)
        let mut refit = Fields::new();
        field(&mut refit, "music_id", "970099");
        package::expand(&package, &mut refit).unwrap();
        assert_eq!(field_str(&refit, "music_id"), "970099");

        wipe(9_100_001);
        wipe(9_100_002);
    }

    #[test]
    fn every_validation_rejection() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_003);
        seed_song(970003, 9_100_003, "public");
        seed_song(970004, 9_100_013, "private");

        let run = |fields: &Fields| create_mv(9_100_003, fields);
        let base = || base_fields(970003, 2, 30);
        let mutated = |key: &str, value: &str| {
            let mut fields = base();
            field(&mut fields, key, value);
            fields
        };

        assert!(run(&mutated("name", "")).unwrap_err().contains("MV name is required"));
        assert!(run(&mutated("name", "<size=400%>x")).unwrap_err().contains("<size>"));
        assert!(run(&mutated("name_en", "<sprite=1>")).unwrap_err().contains("<sprite>"));
        // The song must exist and be the uploader's or public
        assert!(run(&mutated("music_id", "999")).unwrap_err().contains("Unknown music_id"));
        assert!(run(&mutated("music_id", "970004")).unwrap_err().contains("Unknown music_id"));
        assert!(run(&mutated("member_count", "0")).unwrap_err().contains("member_count must be 1-12"));
        assert!(run(&mutated("member_count", "13")).unwrap_err().contains("member_count must be 1-12"));

        // Every slot needs a model and a motion
        let mut fields = base();
        fields.remove("model_2");
        assert!(run(&fields).unwrap_err().contains("'model_2' is required"));
        let mut fields = base();
        fields.remove("motion_2");
        assert!(run(&fields).unwrap_err().contains("'motion_2' is required"));

        // VMD structure: garbage, wrong magic, truncation
        let mut fields = base();
        fields.insert(String::from("motion_1"), b"not a vmd".to_vec());
        assert!(run(&fields).unwrap_err().contains("VMD"));
        let mut fields = base();
        fields.insert(String::from("motion_1"), test_vmd(31)[..60].to_vec());
        assert!(run(&fields).unwrap_err().contains("truncated"));
        let mut fields = base();
        let mut v1 = b"Vocaloid Motion Data file".to_vec();
        v1.resize(60, 0);
        fields.insert(String::from("camera"), v1);
        assert!(run(&fields).unwrap_err().contains("version 1"));

        // Model zip: not a zip, no pmx entry, bad magic, unsupported version
        let mut fields = base();
        fields.insert(String::from("model_1"), b"definitely not a zip".to_vec());
        assert!(run(&fields).unwrap_err().contains("not a valid zip"));
        let mut fields = base();
        fields.insert(String::from("model_1"), zip_with("readme.txt", b"no model here"));
        assert!(run(&fields).unwrap_err().contains("no .pmx model entry"));
        let mut fields = base();
        fields.insert(String::from("model_1"), zip_with("model.pmx", b"XMP 1234abcd"));
        assert!(run(&fields).unwrap_err().contains("missing the \"PMX \" magic"));
        let mut fields = base();
        fields.insert(String::from("model_1"), test_pmx_zip(32, 1.0));
        assert!(run(&fields).unwrap_err().contains("only 2.0 and 2.1"));
        // 2.1 is fine (deleted right away to keep the owner count honest)
        let mut fields = base();
        fields.insert(String::from("model_1"), test_pmx_zip(33, 2.1));
        let ok_id = run(&fields).unwrap();
        delete_mv(9_100_003, ok_id).unwrap();

        // Config must parse as JSON (schema is the client's business, except
        // "stage", which must be one of the in-game stage scene names, and
        // "stage_scale", a number in the client's clamp range)
        let mut fields = base();
        fields.insert(String::from("config"), b"{not json".to_vec());
        assert!(run(&fields).unwrap_err().contains("not valid JSON"));
        let mut fields = base();
        fields.insert(String::from("config"), br#"{"stage":"bg9999_01_s1"}"#.to_vec());
        assert!(run(&fields).unwrap_err().contains("unknown stage"));
        let mut fields = base();
        fields.insert(String::from("config"), br#"{"stage":7}"#.to_vec());
        assert!(run(&fields).unwrap_err().contains("unknown stage"));
        for bad_scale in [r#""big""#, "0.004", "1.5"] {
            let mut fields = base();
            fields.insert(String::from("config"), format!(r#"{{"stage_scale":{}}}"#, bad_scale).into_bytes());
            assert!(run(&fields).unwrap_err().contains("stage_scale must be a number between"), "stage_scale {}", bad_scale);
        }

        // A custom stage must be a PMX zip like a model, plus actual geometry
        let mut fields = base();
        fields.insert(String::from("stage"), b"definitely not a zip".to_vec());
        assert!(run(&fields).unwrap_err().contains("not a valid zip"));
        let mut fields = base();
        fields.insert(String::from("stage"), zip_with("props.txt", b"no model here"));
        assert!(run(&fields).unwrap_err().contains("no .pmx model entry"));
        let mut fields = base();
        fields.insert(String::from("stage"), test_stage_zip(36, 0));
        assert!(run(&fields).unwrap_err().contains("no vertices"));
        // A header that ends before the vertex count (the model-role fixture
        // stops right after the version) is malformed as a stage
        let mut fields = base();
        fields.insert(String::from("stage"), test_pmx_zip(37, 2.0));
        assert!(run(&fields).unwrap_err().contains("malformed PMX header"));
        // A recognized stage passes (deleted right away like the 2.1 model)
        let mut fields = base();
        fields.insert(String::from("config"), br#"{"stage":"bg0008_01_s1","scale":1.0}"#.to_vec());
        let ok_id = run(&fields).unwrap();
        delete_mv(9_100_003, ok_id).unwrap();

        // Only the deliberate successes above ever wrote a row
        assert_eq!(database::mv_count_for_owner(9_100_003), 0);
        wipe(9_100_003);
    }

    // Managing MVs is permissionless beyond login (like custom songs): any
    // user creates/publishes their own, nobody without 3dmv.edit touches
    // someone else's
    #[test]
    fn ownership_gates_and_moderation() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_004);
        wipe(9_100_005);
        seed_song(970005, 9_100_004, "public");

        // Publishing at create needs nothing but the login either
        let mut published_fields = base_fields(970005, 1, 45);
        field(&mut published_fields, "published", "1");
        let published_id = create_mv(9_100_004, &published_fields).unwrap();
        assert!(database::is_published(published_id));

        let id = create_mv(9_100_004, &base_fields(970005, 1, 40)).unwrap();

        // A stranger (no scopes) can't touch someone else's
        let mut edit = Fields::new();
        field(&mut edit, "name", "Hijacked");
        assert!(update_mv(9_100_005, id, &edit).unwrap_err().contains("only manage your own"));
        assert!(delete_mv(9_100_005, id).unwrap_err().contains("only manage your own"));
        assert!(set_mv_flags(9_100_005, id, true).unwrap_err().contains("only manage your own"));

        // 3dmv.edit is moderation: manage ANY MV
        with_permissions(9_100_005, &[permissions::MV_EDIT], || {
            update_mv(9_100_005, id, &edit).unwrap();
            set_mv_flags(9_100_005, id, true).unwrap();
            set_mv_flags(9_100_005, id, false).unwrap();
        });
        assert_eq!(database::get_mv(id).unwrap()["name"].to_string(), "Hijacked");
        with_permissions(9_100_005, &[permissions::MV_EDIT], || delete_mv(9_100_005, id).unwrap());
        assert!(database::get_mv(id).is_none());

        wipe(9_100_004);
        wipe(9_100_005);
    }

    // With the upload permission gone, the login session is the only gate on
    // the management endpoints: a sessionless request is rejected before any
    // form field is even parsed
    #[test]
    fn not_logged_in_is_rejected() {
        let _lock = crate::runtime::lock_test_data_path();
        actix_web::rt::System::new().block_on(async {
            let body_of = |resp: HttpResponse| async {
                let bytes = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
                String::from_utf8_lossy(&bytes).to_string()
            };
            let (req, mut payload) = actix_web::test::TestRequest::default().to_http_parts();
            let multipart = <Multipart as actix_web::FromRequest>::from_request(&req, &mut payload).await.unwrap();
            assert!(body_of(upload(actix_web::test::TestRequest::default().to_http_request(), multipart).await).await.contains("Not logged in"));
            let (req, mut payload) = actix_web::test::TestRequest::default().to_http_parts();
            let multipart = <Multipart as actix_web::FromRequest>::from_request(&req, &mut payload).await.unwrap();
            assert!(body_of(update(actix_web::test::TestRequest::default().to_http_request(), multipart).await).await.contains("Not logged in"));
            let publish_body = jzon::stringify(object!{ mv_id: 1, published: true });
            assert!(body_of(publish(actix_web::test::TestRequest::default().to_http_request(), publish_body).await).await.contains("Not logged in"));
            let delete_body = jzon::stringify(object!{ mv_id: 1 });
            assert!(body_of(delete(actix_web::test::TestRequest::default().to_http_request(), delete_body).await).await.contains("Not logged in"));
            assert!(body_of(mine(actix_web::test::TestRequest::default().to_http_request()).await).await.contains("Not logged in"));
        });
    }

    // Present files replace (old blobs GC), absent files keep, `_delete`
    // drops optional roles, and the slot-completeness rule holds for the
    // resulting member_count
    #[test]
    fn update_keeps_and_deletes_files() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_006);
        seed_song(970006, 9_100_006, "public");
        let upload = |fields: &Fields| create_mv(9_100_006, fields);
        let edit = |id: i64, fields: &Fields| update_mv(9_100_006, id, fields);

        let id = upload(&base_fields(970006, 2, 50)).unwrap();
        let before = database::get_mv(id).unwrap();
        let old_motion = file_md5(&before, "motion", Some(1));
        let old_model = file_md5(&before, "model", Some(1));

        // Replace one motion, rename; everything else keeps
        let mut fields = Fields::new();
        field(&mut fields, "name", "Renamed");
        fields.insert(String::from("motion_1"), test_vmd(60));
        edit(id, &fields).unwrap();
        let after = database::get_mv(id).unwrap();
        assert_eq!(after["name"].as_str(), Some("Renamed"));
        assert_eq!(after["name_en"], before["name_en"]);
        assert_eq!(after["member_count"].as_i64(), Some(2));
        assert_eq!(after["music_id"], before["music_id"]);
        let new_motion = file_md5(&after, "motion", Some(1));
        assert_ne!(new_motion, old_motion);
        assert_eq!(file_md5(&after, "model", Some(1)), old_model);
        // The replaced blob is gone (nothing else references it), the new and
        // the kept ones exist
        assert!(fs::read(blob_path(&old_motion)).is_err());
        assert!(!database::find_blob_by_md5(&old_motion));
        assert!(fs::read(blob_path(&new_motion)).is_ok());
        assert!(fs::read(blob_path(&old_model)).is_ok());

        // Optional roles delete by flag; blobs follow
        let facial = file_md5(&after, "facial", Some(1));
        let camera = file_md5(&after, "camera", None);
        let mut fields = Fields::new();
        field(&mut fields, "facial_1_delete", "1");
        field(&mut fields, "camera_delete", "1");
        edit(id, &fields).unwrap();
        let after = database::get_mv(id).unwrap();
        assert!(!after["files"].members().any(|f| f["role"] == "facial"));
        assert!(!after["files"].members().any(|f| f["role"] == "camera"));
        assert!(fs::read(blob_path(&facial)).is_err());
        assert!(fs::read(blob_path(&camera)).is_err());

        // Replace + delete on the same file is contradictory; required roles
        // can't be deleted at all
        let mut fields = Fields::new();
        field(&mut fields, "camera_delete", "1");
        fields.insert(String::from("camera"), test_camera_vmd(55));
        assert!(edit(id, &fields).unwrap_err().contains("cannot both replace and delete"));
        let mut fields = Fields::new();
        field(&mut fields, "model_1_delete", "1");
        assert!(edit(id, &fields).unwrap_err().contains("cannot be deleted"));

        // Shrinking member_count drops the higher slots and their blobs
        let slot2_model = file_md5(&database::get_mv(id).unwrap(), "model", Some(2));
        let mut fields = Fields::new();
        field(&mut fields, "member_count", "1");
        edit(id, &fields).unwrap();
        let after = database::get_mv(id).unwrap();
        assert!(!after["files"].members().any(|f| f["slot"] == 2));
        assert!(fs::read(blob_path(&slot2_model)).is_err());

        // Growing it back demands the new slots' files
        let mut fields = Fields::new();
        field(&mut fields, "member_count", "2");
        assert!(edit(id, &fields).unwrap_err().contains("'model_2' is required"));
        // A rejected edit wrote nothing
        assert_eq!(database::get_mv(id).unwrap()["member_count"].as_i64(), Some(1));

        wipe(9_100_006);
    }

    // The optional slot-less "stage" role: a custom PMX stage carried exactly
    // like camera/config, with the same keep/replace/delete semantics, plus
    // the stage_scale boundary values and the package round-trip
    #[test]
    fn stage_role_upload_update_and_package() {
        let _lock = crate::runtime::lock_test_data_path();
        let uid = 9_100_012;
        wipe(uid);
        seed_song(970041, uid, "public");

        let mut fields = base_fields(970041, 1, 110);
        fields.insert(String::from("stage"), test_stage_zip(111, 42));
        fields.insert(String::from("config"), br#"{"stage":"bg0008_01_s1","stage_scale":0.08}"#.to_vec());
        let id = create_mv(uid, &fields).unwrap();

        // The catalog carries a slot-less stage entry addressing the exact
        // bytes the data route serves
        let mv = database::get_mv(id).unwrap();
        let entry = mv["files"].members().find(|f| f["role"] == "stage").unwrap();
        assert!(entry["slot"].is_null());
        let md5 = entry["md5"].to_string();
        let bytes = fs::read(blob_path(&md5)).unwrap();
        assert_eq!(format!("{:x}", md5::compute(&bytes)), md5);
        assert_eq!(bytes.len(), entry["size"].as_usize().unwrap());
        assert!(database::find_blob_by_md5(&md5));

        // The clamp-range boundaries are valid stage_scale values
        for scale in ["0.005", "1"] {
            let mut edit = Fields::new();
            edit.insert(String::from("config"), format!(r#"{{"stage_scale":{}}}"#, scale).into_bytes());
            update_mv(uid, id, &edit).unwrap();
        }

        // An absent field keeps the stored stage; a new file replaces it and
        // the old blob GCs
        let mut edit = Fields::new();
        field(&mut edit, "name", "Renamed");
        update_mv(uid, id, &edit).unwrap();
        assert_eq!(file_md5(&database::get_mv(id).unwrap(), "stage", None), md5);
        let mut edit = Fields::new();
        edit.insert(String::from("stage"), test_stage_zip(112, 7));
        update_mv(uid, id, &edit).unwrap();
        let new_md5 = file_md5(&database::get_mv(id).unwrap(), "stage", None);
        assert_ne!(new_md5, md5);
        assert!(fs::read(blob_path(&md5)).is_err());
        assert!(fs::read(blob_path(&new_md5)).is_ok());

        // The export package carries the stage blob and expands it back onto
        // the same field name
        let package = package::build(id).unwrap();
        let mut expanded = Fields::new();
        package::expand(&package, &mut expanded).unwrap();
        assert_eq!(expanded.get("stage"), Some(&test_stage_zip(112, 7)));

        // Replace + delete is contradictory; a plain stage_delete drops the
        // role and its blob
        let mut edit = Fields::new();
        field(&mut edit, "stage_delete", "1");
        edit.insert(String::from("stage"), test_stage_zip(113, 5));
        assert!(update_mv(uid, id, &edit).unwrap_err().contains("cannot both replace and delete"));
        let mut edit = Fields::new();
        field(&mut edit, "stage_delete", "1");
        update_mv(uid, id, &edit).unwrap();
        assert!(!database::get_mv(id).unwrap()["files"].members().any(|f| f["role"] == "stage"));
        assert!(fs::read(blob_path(&new_md5)).is_err());

        wipe(uid);
    }

    // The referential closure: a published MV is only served to users whose
    // OWN song catalog delivers its music_id
    #[test]
    fn catalog_closure_follows_song_visibility() {
        let _lock = crate::runtime::lock_test_data_path();
        let owner = 9_100_007;
        let stranger = 9_100_008;
        wipe(owner);
        seed_song(970011, owner, "public");
        seed_song(970012, owner, "private");

        let mut fields = base_fields(970011, 1, 100);
        field(&mut fields, "published", "1");
        let public_song_mv = create_mv(owner, &fields).unwrap();
        let mut fields = base_fields(970012, 1, 105);
        field(&mut fields, "published", "1");
        let private_song_mv = create_mv(owner, &fields).unwrap();

        // The owner's song catalog carries both songs, so both MVs resolve
        let owner_catalog = catalog_for_user(owner);
        assert!(owner_catalog.members().any(|m| m["mv_id"] == public_song_mv));
        assert!(owner_catalog.members().any(|m| m["mv_id"] == private_song_mv));
        // A stranger's catalog delivers only the public song - the published
        // MV on the private song must NOT be served
        let stranger_catalog = catalog_for_user(stranger);
        assert!(stranger_catalog.members().any(|m| m["mv_id"] == public_song_mv));
        assert!(!stranger_catalog.members().any(|m| m["mv_id"] == private_song_mv));

        // Sharing the song brings its MV along
        crate::database::custom_song::set_visibility(970012, "shared", &array![stranger]).unwrap();
        assert!(catalog_for_user(stranger).members().any(|m| m["mv_id"] == private_song_mv));
        crate::database::custom_song::set_visibility(970012, "private", &array![]).unwrap();

        wipe(owner);
    }

    // Deletion GC keeps shared blobs alive, dead_mv_ids reports only deleted
    // band ids, and the song-delete cascade purges the song's MVs
    #[test]
    fn delete_gc_cascade_and_dead_ids() {
        let _lock = crate::runtime::lock_test_data_path();
        let uid = 9_100_009;
        wipe(uid);
        seed_song(970021, uid, "public");
        seed_song(970022, uid, "public");

        // Two MVs sharing one motion blob (content-addressed store)
        let shared_vmd = test_vmd(70);
        let shared_md5 = format!("{:x}", md5::compute(&shared_vmd));
        let mut f1 = base_fields(970021, 1, 71);
        f1.insert(String::from("motion_1"), shared_vmd.clone());
        let mut f2 = base_fields(970022, 1, 76);
        f2.insert(String::from("motion_1"), shared_vmd.clone());
        let id1 = create_mv(uid, &f1).unwrap();
        let id2 = create_mv(uid, &f2).unwrap();
        let model1 = file_md5(&database::get_mv(id1).unwrap(), "model", Some(1));

        delete_mv(uid, id1).unwrap();
        assert!(database::get_mv(id1).is_none());
        // The shared blob survives (id2 still references it), the unique one
        // is gone
        assert!(fs::read(blob_path(&shared_md5)).is_ok());
        assert!(fs::read(blob_path(&model1)).is_err());

        // Deleted band ids come back; alive and out-of-band ids never do
        let dead = database::dead_mv_ids(&array![id1, id2, 15000, 100000, id1]);
        assert_eq!(dead.len(), 1);
        assert_eq!(dead[0].as_i64(), Some(id1));

        // The cascade: purging the song deletes its MV, GCs its blobs and
        // bumps the revision once
        let revision = database::get_revision();
        purge_song(970022);
        assert!(database::get_mv(id2).is_none());
        assert!(fs::read(blob_path(&shared_md5)).is_err());
        assert_eq!(database::get_revision(), revision + 1);
        assert!(database::dead_mv_ids(&array![id2]).contains(id2));
        // A song with no MVs purges to a no-op, revision untouched
        purge_song(970021);
        assert_eq!(database::get_revision(), revision + 1);

        wipe(uid);
    }

    // The startup sweep removes exactly the orphans: referenced blobs and
    // non-{md5}.bin names stay
    #[test]
    fn sweep_removes_only_orphan_blobs() {
        let _lock = crate::runtime::lock_test_data_path();
        let uid = 9_100_010;
        wipe(uid);
        seed_song(970031, uid, "public");
        let id = create_mv(uid, &base_fields(970031, 1, 80)).unwrap();
        let mv = database::get_mv(id).unwrap();

        let orphan = blob_path(&"a".repeat(32));
        fs::write(&orphan, b"orphaned by an interrupted upload").unwrap();
        let junk = get_data_path("custom_3dmv/blobs/notahash.bin");
        fs::write(&junk, b"not ours to manage").unwrap();

        sweep_blobs();

        assert!(fs::read(&orphan).is_err());
        assert!(fs::read(&junk).is_ok());
        for file in mv["files"].members() {
            assert!(fs::read(blob_path(&file["md5"].to_string())).is_ok(), "referenced blob {} must survive", file["md5"]);
        }
        let _ = fs::remove_file(&junk);
        wipe(uid);
    }

    // ---- defect-fix coverage -------------------------------------------------

    // A stage PMX whose header declares a text field of `len` bytes, with the
    // bytes actually present
    fn stage_zip_with_text_len(len: i32, present: usize) -> Vec<u8> {
        let mut pmx = Vec::new();
        pmx.extend(b"PMX ");
        pmx.extend(2.0f32.to_le_bytes());
        pmx.push(8);
        pmx.extend([0u8; 8]);
        // The model name carries the declared length; the other three are empty
        pmx.extend(len.to_le_bytes());
        pmx.extend(vec![0u8; present]);
        for _ in 0..3 {
            pmx.extend(0u32.to_le_bytes());
        }
        pmx.extend(1i32.to_le_bytes());
        zip_with("stage.pmx", &pmx)
    }

    // D11: the length prefix of a PMX text field is an attacker-supplied i32 the
    // stage walk skips over. Uncapped it inflated up to 2GB into a sink, four
    // times per entry - CPU with nothing to show for it
    #[test]
    fn pmx_text_fields_are_bounded() {
        let big = MAX_PMX_TEXT_BYTES as usize;
        // At the cap, with the bytes really there: a valid stage
        assert!(validate_file("stage", "stage", &stage_zip_with_text_len(big as i32, big)).is_ok());
        // One byte over, bytes present: refused rather than skipped over
        let err = validate_file("stage", "stage", &stage_zip_with_text_len(big as i32 + 1, big + 1)).unwrap_err();
        assert!(err.contains("malformed PMX header"), "{}", err);
        // And the classic: a huge declaration with nothing behind it
        let err = validate_file("stage", "stage", &stage_zip_with_text_len(i32::MAX, 0)).unwrap_err();
        assert!(err.contains("malformed PMX header"), "{}", err);
    }

    // D1: a package entry is capped as it inflates. Deflate's ~1032:1 ceiling
    // means an uncapped read_to_end turns a tiny zip into gigabytes, and a package
    // carries 39 addressable entries
    #[test]
    fn package_import_is_capped() {
        let mut zip = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let options = zip::write::SimpleFileOptions::default();
        zip.start_file("manifest.json", options).unwrap();
        zip.write_all(jzon::stringify(object!{
            "format": 1, "name": "Bomb", "name_en": "Bomb", "music_id": 970050, "member_count": 1
        }).as_bytes()).unwrap();
        // Compresses to a few KB, inflates to just over the per-file cap
        zip.start_file("model_1", options).unwrap();
        zip.write_all(&vec![0u8; MAX_FILE_BYTES + 1]).unwrap();
        let package = zip.finish().unwrap().into_inner();
        assert!(package.len() < 1024 * 1024, "the bomb should be small: {} bytes", package.len());

        let mut fields = Fields::new();
        let err = package::expand(&package, &mut fields).unwrap_err();
        assert!(err.contains("per-file limit"), "{}", err);
        assert!(fields.get("model_1").is_none(), "the oversized entry was buffered anyway");
    }

    // D1/D6: the per-request total is accounted over the whole form, which is what
    // a package's expanded contents are re-checked against
    #[test]
    fn the_request_total_is_capped() {
        let mut fields = Fields::new();
        fields.insert(String::from("a"), vec![0; MAX_FILE_BYTES]);
        assert!(check_field_caps(&fields).is_ok());
        let mut one = Fields::new();
        one.insert(String::from("model_1"), vec![0; MAX_FILE_BYTES + 1]);
        assert!(check_field_caps(&one).unwrap_err().contains("per-file limit"));
    }

    // D2: a read error must never read as "not referenced". The GC unlinks on
    // false, so a failed lookup has to mean "assume in use" - the startup sweep
    // has always been fail-closed and the online path now matches it
    #[test]
    fn a_database_error_never_deletes_a_blob() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_030);
        seed_song(970201, 9_100_030, "public");
        let id = create_mv(9_100_030, &base_fields(970201, 1, 120)).unwrap();
        let stored = database::get_mv(id).unwrap();
        let md5 = stored["files"][0]["md5"].to_string();
        assert_eq!(md5.len(), 32);
        assert_eq!(database::blob_in_use(&md5), Ok(true));
        assert_eq!(database::blob_in_use(&"c7".repeat(16)), Ok(false));

        let conn = rusqlite::Connection::open(database::test_db_path()).unwrap();
        conn.execute("ALTER TABLE mvs RENAME TO mvs_hidden", ()).unwrap();
        assert!(database::blob_in_use(&md5).is_err());
        // The blob is still live; the GC must keep what it cannot prove is orphaned
        gc_blobs(&stored["files"]);
        conn.execute("ALTER TABLE mvs_hidden RENAME TO mvs", ()).unwrap();

        assert!(fs::read(blob_path(&md5)).is_ok(), "a live blob was unlinked on a read error");
        wipe(9_100_030);
    }

    // D9: every catalog closes over the songs the viewer can see, so a published
    // MV attached to someone else's PRIVATE song is invisible everywhere - and it
    // must not be downloadable by walking mv_ids either
    #[test]
    fn download_closes_over_song_visibility() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_031);
        seed_song(970202, 9_100_031, "private");
        seed_song(970203, 9_100_031, "public");
        let hidden = create_mv(9_100_031, &base_fields(970202, 1, 130)).unwrap();
        let open = create_mv(9_100_031, &base_fields(970203, 1, 140)).unwrap();
        set_mv_flags(9_100_031, hidden, true).unwrap();
        set_mv_flags(9_100_031, open, true).unwrap();

        let call = |mv_id: i64| -> String {
            let req = actix_web::test::TestRequest::default().param("mv_id", mv_id.to_string()).to_http_request();
            actix_web::rt::System::new().block_on(async {
                let resp = download(req).await;
                let bytes = actix_web::body::to_bytes(resp.into_body()).await.unwrap();
                String::from_utf8_lossy(&bytes).to_string()
            })
        };
        // Published, on a song an anonymous viewer's catalog never delivers
        assert!(call(hidden).contains("MV not found"), "a private song's MV was downloadable");
        // Published, on a public song: still downloadable
        assert!(!call(open).contains("MV not found"));

        wipe(9_100_031);
    }

    // D15: the quota counts the stored file bytes the catalog quotes, per owner
    #[test]
    fn uploads_are_bounded_by_a_per_account_byte_quota() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(9_100_032);
        assert_eq!(database::owner_bytes(9_100_032, 0), 0);
        seed_song(970204, 9_100_032, "public");
        let id = create_mv(9_100_032, &base_fields(970204, 1, 150)).unwrap();

        let stored = database::mv_bytes(&database::get_mv(id).unwrap()["files"]);
        assert!(stored > 0);
        assert_eq!(database::owner_bytes(9_100_032, 0), stored);
        // An in-place edit replaces its own bytes rather than adding to them
        assert_eq!(database::owner_bytes(9_100_032, id), 0);

        assert!(check_quota(9_100_032, 1, 0).is_ok());
        assert!(check_quota(9_100_032, MAX_BYTES_PER_USER, 0).unwrap_err().contains("per-account limit"));
        assert_eq!(database::owner_bytes(9_100_033, 0), 0);

        wipe(9_100_032);
    }

}

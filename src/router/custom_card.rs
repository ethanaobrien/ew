mod art;

use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, HttpResponse, Responder, http::header::ContentType};
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use lazy_static::lazy_static;
use std::collections::{HashMap, HashSet};
use std::fs;
use std::sync::Mutex;

use crate::router::{databases, global, userdata, webui, Login, Api};
use crate::router::databases::csv::{table, Region};
use crate::router::custom_song::audio;
use crate::database::custom_card as database;
use crate::database::permissions;
use crate::runtime::get_data_path;
use crate::lock_onto_mutex;

// Runtime-uploaded cards and characters. Unlike the SIF1 import (id prefixes
// 10000-14999, baked into client masterdata at build time) these rows do not
// exist in any shipped table: the client fetches them from
// /api/custom_card/list at login (protocol version 3) and appends them to its
// Mst tables before the first payload carrying card_list arrives.
//
// Cards are owned by their uploader. Draft by default: a draft is served to
// its owner's catalog only. Publishing puts it in everyone's catalog, and
// "obtainable" additionally enters it into the client-synthesized custom
// gacha banner (lottery id 6900001), whose draw lottery.rs special-cases.
// Filtering is at the CATALOG level; the art GET is content-addressed and
// sessionless, like a CDN.
//
// Storage layout (under --path):
//   custom_cards/{master_card_id}/{kind}_{variant}.png   c_00.png ... sc_01.png
//   custom_cards/characters/{master_character_id}/{kind}.png
// Metadata lives in custom_cards.db as one JSON blob per card / character, in
// the exact shape /api/custom_card/list serves.

// Level 1 = custom songs, 2 = resolves the baked SIF1-import band, 3 = fetches
// the runtime custom-card catalog
pub const PROTOCOL_VERSION: u32 = 3;

// The client-synthesized custom gacha banner. The 6M band is reserved for
// custom lotteries (the baked SIF1 banners are 6110001-6110004)
pub const CUSTOM_LOTTERY_ID: i64 = 6_900_001;

// Upload limits, enforced while the multipart field is still streaming (the
// 25MB PayloadConfig in lib.rs binds the String/Bytes extractors, not
// Multipart). A card upload carries 14 png files, so the per-request cap is
// the binding one
pub const MAX_FILE_BYTES: usize = 8 * 1024 * 1024;
pub const MAX_REQUEST_BYTES: usize = 64 * 1024 * 1024;
pub const MAX_CARDS_PER_USER: i64 = 500;

// Columns the uploader never supplies. master_release_label_id must be 1: a
// closed label filters the card out of the member-list filters and drops its
// evolve conditions
const MASTER_RELEASE_LABEL_ID: i64 = 1;
// card_get.csv category 1 = GACHA, matching how the card is actually obtained
const GET_CATEGORY_GACHA: i64 = 1;

// Every one of the 172 imported characters carries exactly these values
// (csv/character.csv), and category 6 (BAND_CATEGORY OTHER) is not cosmetic -
// SDCharacter substitutes the 99999 atlas for an OTHER-category character,
// which is what gives a new character a working SD chibi with no new asset.
// master_group_id 9000 is a real group id in character_group.csv - a
// nonexistent group id is the class of bug that KeyNotFound-crashed custom
// songs. These are NUMBERS on the wire, never enum names
const CHARACTER_CATEGORY_OTHER: i64 = 6;
const CHARACTER_CHARA_CATEGORY: i64 = 1;
const CHARACTER_GROUP_ID: i64 = 9000;
const CHARACTER_SCHOOL_GRADE: i64 = 0;

// Client enum ranges (crash ranges, not cosmetic: the member sorter indexes
// arrays sized to the enum with the raw value). Kept as named consts in ONE
// place; the client agent is re-verifying the exact ceilings
// 0 = NONE exists in the client enum but no shipped skill row uses it (a NONE
// live skill would do nothing), so uploads start at 1
const SKILL_EFFECT_TYPE_MIN: i64 = 1;
const SKILL_EFFECT_TYPE_MAX: i64 = 11;
const SKILL_TRIGGER_MIN: i64 = 1;
const SKILL_TRIGGER_MAX: i64 = 4;
const SKILL_SUB_TARGET_MAX: i64 = 1;
const SKILL_SCHOOL_GRADE_MAX: i64 = 3;
const CARD_TYPE_MIN: i64 = 1;
const CARD_TYPE_MAX: i64 = 4;
const CARD_RARITY_MIN: i64 = 1;
const CARD_RARITY_MAX: i64 = 3;
const RARITY_NAMES: &[&str] = &["R", "SR", "UR"];
// Sanity ceilings for the level-indexed skill arrays
const SKILL_PROBABILITY_MAX: i64 = 1_000_000;
const SKILL_MILLI_SECS_MAX: i64 = 600_000;

struct ArtKind {
    kind: &'static str,
    width: u32,
    height: u32
}

// Official target dimensions, verified against the shipped art. Every kind is
// DERIVED from the per-variant source artwork (art_00 / art_01) by the SIF1
// import pipeline's recipes (see art.rs); an explicitly supplied kind file
// overrides the derived one and is itself cover-cropped + resized to target,
// never rejected for dimensions. The stored/hashed bytes are always the
// processed PNG
const CARD_ART: &[ArtKind] = &[
    ArtKind { kind: "c",  width: 2048, height: 1260 },
    ArtKind { kind: "h",  width: 2048, height: 1260 },
    ArtKind { kind: "t",  width: 512,  height: 315  },
    ArtKind { kind: "p",  width: 136,  height: 508  },
    ArtKind { kind: "r",  width: 256,  height: 256  },
    ArtKind { kind: "m",  width: 380,  height: 380  },
    ArtKind { kind: "sc", width: 1024, height: 512  }
];

// "00" = base, "01" = evolved. Both are required for every kind: the client
// picks evolve ? evolve_illust_id : illust_id with no rarity gate
const CARD_ART_VARIANTS: &[&str] = &["00", "01"];

const CHARACTER_ART: &[ArtKind] = &[
    ArtKind { kind: "pr",        width: 512, height: 615 },
    ArtKind { kind: "icon",      width: 230, height: 230 },
    ArtKind { kind: "sign",      width: 300, height: 330 },
    ArtKind { kind: "character", width: 600, height: 920 }
];

// Optional voicelines per character. Multipart fields per line:
//   voice_{kind}_{index}          audio file (any format symphonia reads;
//                                 transcoded to ogg-vorbis, stored
//                                 content-addressed like the art)
//   voice_{kind}_{index}_text     caption, may be empty
//   voice_{kind}_{index}_text_en  English caption, may be empty
//   voice_{kind}_{index}_delete   flag: remove this line
// Absent slots keep their stored line, captions without a file update the
// stored line's captions, and surviving lines are renumbered contiguously
// per kind (1..n) after every edit
const VOICE_KINDS: &[&str] = &[
    "live_start", "live_success", "live_failed", "result_bond",
    "skill_smile", "skill_pure", "skill_cool"
];
const MAX_VOICE_VARIANTS: usize = 9;
const MAX_VOICE_BYTES: usize = 4 * 1024 * 1024;
const MAX_VOICE_SECONDS: f64 = 30.0;

type Fields = HashMap<String, Vec<u8>>;

lazy_static! {
    // Id allocation and the insert must not race between two uploads
    static ref UPLOAD_LOCK: Mutex<()> = Mutex::new(());

    static ref OFFICIAL_CHARACTER_IDS: HashSet<i64> = {
        table(Region::Jp, "character").members().filter_map(|row| row["id"].as_i64()).collect()
    };

    static ref SKILL_CENTER_IDS: HashSet<i64> = {
        table(Region::Jp, "skill_center").members().filter_map(|row| row["id"].as_i64()).collect()
    };

    // Real GroupMst ids, from the character->group mapping. skill
    // target_group_id must be 0 or one of these
    static ref GROUP_IDS: HashSet<i64> = {
        table(Region::Jp, "character_group").members().filter_map(|row| row["groupId"].as_i64()).collect()
    };

    // rarity -> how many skill levels its curve has (3 / 5 / 9). The client
    // indexes the level arrays in lockstep with the skill level, so this is
    // the required length of every level-indexed array
    static ref SKILL_LEVEL_COUNT: HashMap<i64, usize> = {
        let mut per_curve: HashMap<i64, usize> = HashMap::new();
        for row in table(Region::Jp, "card_skill_level").members() {
            if let Some(id) = row["id"].as_i64() {
                *per_curve.entry(id).or_insert(0) += 1;
            }
        }
        let mut rv = HashMap::new();
        for row in table(Region::Jp, "card_rarity").members() {
            let (Some(rarity), Some(curve)) = (row["rarity"].as_i64(), row["masterCardSkillLevelId"].as_i64()) else { continue; };
            rv.insert(rarity, *per_curve.get(&curve).unwrap_or(&0));
        }
        rv
    };

    // rarity -> (hp, smile, cool, pure) ceilings: the maximum any official
    // card of that rarity reaches. do_reinforce trusts the stored card
    // completely, so an uploaded stat is permanent - cap it at upload
    static ref STAT_CAPS: HashMap<i64, (i64, i64, i64, i64)> = {
        let mut rv: HashMap<i64, (i64, i64, i64, i64)> = HashMap::new();
        for row in databases::CARD_LIST.entries() {
            let card = row.1;
            let Some(id) = card["id"].as_i64() else { continue; };
            // The imported band shares official stat scales; both are fine as
            // a ceiling source, runtime cards are excluded by construction
            if id >= database::FIRST_CARD_ID {
                continue;
            }
            let Some(rarity) = card["rarity"].as_i64() else { continue; };
            let entry = rv.entry(rarity).or_insert((0, 0, 0, 0));
            entry.0 = entry.0.max(card["hp"].as_i64().unwrap_or(0));
            entry.1 = entry.1.max(card["smile"].as_i64().unwrap_or(0));
            entry.2 = entry.2.max(card["cool"].as_i64().unwrap_or(0));
            entry.3 = entry.3.max(card["pure"].as_i64().unwrap_or(0));
        }
        rv
    };
}

// Game endpoints (/api scope, standard envelope)
pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/custom_card")
            .route("/list", web::post().to(list))
    );
}

// Plain art GET for the game + session-authenticated management API for the
// webui. Mounted OUTSIDE /api so the game middlewares never wrap it
pub fn web_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/custom_card")
            .route("/data/{hash}/{file}", web::get().to(data))
            .route("/voice/{hash}/{file}", web::get().to(voice))
            .route("/create", web::post().to(create))
            .route("/update", web::post().to(update))
            .route("/publish", web::post().to(publish))
            .route("/delete", web::post().to(delete))
            .route("/mine", web::get().to(mine))
            .route("/browse", web::get().to(browse))
            .route("/character/create", web::post().to(character_create))
            .route("/character/update", web::post().to(character_update))
            .route("/character/delete", web::post().to(character_delete))
    );
}

// The whole feature is opt-in (--enable-custom-cards) and additionally off in
// --hidden mode. When disabled every endpoint 404s / errors as if it never
// existed, nothing touches custom_cards.db (so no table setup runs), and no
// runtime card ever resolves
pub fn disabled() -> bool {
    let args = crate::get_args();
    args.hidden || !args.enable_custom_cards
}

// The runtime-uploaded band. card::is_custom covers 100M+ (baked import
// included); this is the narrower "not in any shipped masterdata" test
pub fn is_custom_runtime(master_card_id: i64) -> bool {
    (database::FIRST_CARD_ID..=database::LAST_CARD_ID).contains(&master_card_id)
}

pub fn client_supports(req: &HttpRequest) -> bool {
    global::client_protocol_version(req) >= PROTOCOL_VERSION
}

pub fn account_supports(auth_key: &str) -> bool {
    userdata::get_protocol_version(auth_key) >= PROTOCOL_VERSION
}

// Whether a viewer on this protocol version can turn `master_card_id` into a
// card row. "Supports the protocol" is not "has this card in its catalog": a
// draft (or a card published after the viewer logged in) is unresolvable by a
// fully modern client, and the client throws on an unknown id rather than
// degrading. The protocol test comes first so an older viewer never touches
// custom_cards.db
pub fn viewer_can_resolve(master_card_id: i64, protocol: u32) -> bool {
    if !crate::router::card::is_custom(master_card_id) {
        return true;
    }
    if !is_custom_runtime(master_card_id) {
        return protocol >= crate::router::card::PROTOCOL_VERSION;
    }
    if disabled() || protocol < PROTOCOL_VERSION {
        return false;
    }
    database::is_published(master_card_id)
}

// The character a runtime card belongs to, for guest::proxy_card_id
pub fn character_of(master_card_id: i64) -> Option<i64> {
    if disabled() {
        return None;
    }
    database::character_of(master_card_id)
}

// The card row for any master_card_id, in the csv CARD_LIST shape. Official
// and imported cards come straight from the baked table; runtime cards are
// synthesized from the db blob so the CARD_LIST consumers (reinforce, evolve,
// rarity, bond) keep working on them. Null when the id doesn't exist anywhere
pub fn card_info(master_card_id: i64) -> JsonValue {
    let official = &databases::CARD_LIST[master_card_id.to_string()];
    if !official.is_empty() {
        return official.clone();
    }
    if !is_custom_runtime(master_card_id) || disabled() {
        return JsonValue::Null;
    }
    let Some(card) = database::get_card(master_card_id) else {
        return JsonValue::Null;
    };
    object!{
        "id": master_card_id,
        "masterCharacterId": card["master_character_id"].clone(),
        "type": card["type"].clone(),
        "rarity": card["rarity"].clone(),
        "masterCardLevelId": card["master_card_level_id"].clone()
    }
}

// Runtime-band cards are unresolvable by a client that can't fetch the
// catalog, and one unknown id in card_list aborts the whole login. start.rs
// blocks flagged accounts on old clients, so this is belt-and-braces for the
// shared /api/user response
pub fn strip_unsupported(user: &mut JsonValue) {
    let mut dropped = array![];
    let mut card_list = array![];
    for card in user["card_list"].members() {
        let id = card["master_card_id"].as_i64().unwrap_or(0);
        if is_custom_runtime(id) {
            dropped.push(id).unwrap();
            continue;
        }
        card_list.push(card.clone()).unwrap();
    }
    if dropped.is_empty() {
        return;
    }
    user["card_list"] = card_list;
    for deck in user["deck_list"].members_mut() {
        for id in deck["main_card_ids"].members_mut() {
            if dropped.contains(id.as_i64().unwrap_or(0)) {
                *id = (0).into();
            }
        }
    }
    for key in ["favorite_master_card_id", "guest_smile_master_card_id", "guest_cool_master_card_id", "guest_pure_master_card_id"] {
        if dropped.contains(user["user"][key].as_i64().unwrap_or(0)) {
            user["user"][key] = (0).into();
        }
    }
}

// The concrete upload bounds, served to the webui so the form can enforce
// them client-side (sliders/radios) and reject out-of-range values instantly
pub fn upload_limits() -> JsonValue {
    let mut stat_caps = object!{};
    let mut skill_levels = object!{};
    for rarity in CARD_RARITY_MIN..=CARD_RARITY_MAX {
        let caps = STAT_CAPS.get(&rarity).copied().unwrap_or((0, 0, 0, 0));
        stat_caps[rarity.to_string()] = object!{
            "hp": caps.0,
            "smile": caps.1,
            "cool": caps.2,
            "pure": caps.3
        };
        skill_levels[rarity.to_string()] = (*SKILL_LEVEL_COUNT.get(&rarity).unwrap_or(&0)).into();
    }
    // The real groups a skill may target, with display names for the webui's
    // dropdown (nobody knows the raw group ids)
    let mut rows: Vec<(i64, JsonValue)> = table(Region::Jp, "group").members()
        .filter_map(|row| Some((row["id"].as_i64()?, row.clone())))
        .collect();
    rows.sort_by_key(|(id, _)| *id);
    let mut groups = array![];
    for (id, row) in rows {
        groups.push(object!{
            "id": id,
            "name": row["name"].clone(),
            "name_en": row["nameEn"].clone()
        }).unwrap();
    }
    object!{
        "stat_caps": stat_caps,
        "skill_levels": skill_levels,
        "groups": groups,
        "trigger_min": SKILL_TRIGGER_MIN,
        "trigger_max": SKILL_TRIGGER_MAX,
        "effect_type_min": SKILL_EFFECT_TYPE_MIN,
        "effect_type_max": SKILL_EFFECT_TYPE_MAX,
        "sub_target_max": SKILL_SUB_TARGET_MAX,
        "school_grade_max": SKILL_SCHOOL_GRADE_MAX,
        "probability_max": SKILL_PROBABILITY_MAX,
        "milli_secs_max": SKILL_MILLI_SECS_MAX,
        "min_source_dim": art::MIN_SOURCE_DIM
    }
}

// The runtime card ids in a game account's card_list
pub fn owned_runtime_ids(user: &JsonValue) -> Vec<i64> {
    user["card_list"].members()
        .filter_map(|card| card["master_card_id"].as_i64())
        .filter(|id| is_custom_runtime(*id))
        .collect()
}

// The catalog is filtered per requesting user: everyone gets the published
// cards, the owner additionally gets their drafts, and a game account that
// already owns a card keeps resolving it even if it was since unpublished
async fn list(Login(key): Login) -> impl Responder {
    if disabled() {
        // As if the endpoint doesn't exist - the client treats this as feature-off
        return Api(None);
    }
    let user = userdata::get_acc(&key);
    let uid = user["user"]["id"].as_i64().unwrap();
    let cards = database::get_cards_for_user(uid, &owned_runtime_ids(&user));
    let characters = database::get_characters_for_cards(uid, &cards);
    Api(Some(object!{
        "revision": database::get_revision(),
        "characters": characters,
        "cards": cards
    }))
}

fn card_dir(master_card_id: i64) -> String {
    get_data_path(&format!("custom_cards/{}", master_card_id))
}

fn character_dir(master_character_id: i64) -> String {
    get_data_path(&format!("custom_cards/characters/{}", master_character_id))
}

fn asset_path(relative: &str) -> String {
    get_data_path(&format!("custom_cards/{}", relative))
}

// Content-addressed art fetch: '{server}/custom_card/data/{md5}/{md5}.png'.
// The game builds the URL from the md5 it read in the catalog and caches by
// it, so a stale md5 simply 404s and the client re-downloads under the new
// one. Visible to all like the custom-song data route (CDN semantics) - only
// the feature flag gates it
async fn data(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let hash = req.match_info().get("hash").unwrap_or("").to_string();
    let file = req.match_info().get("file").unwrap_or("").to_string();
    if hash.len() != 32 || !hash.chars().all(|c| c.is_ascii_hexdigit()) || !file.starts_with(&format!("{}.", hash)) {
        return HttpResponse::NotFound().finish();
    }
    let Some(relative) = database::find_asset_by_md5(&hash) else {
        return HttpResponse::NotFound().finish();
    };
    match fs::read(asset_path(&relative)) {
        Ok(body) => {
            HttpResponse::Ok()
                .insert_header(ContentType::png())
                .insert_header(("content-length", body.len()))
                .body(body)
        },
        Err(_) => HttpResponse::NotFound().finish()
    }
}

// Voiceline oggs, same sessionless content-addressed semantics as the art
// data route: '{server}/custom_card/voice/{md5}/{md5}.ogg'
async fn voice(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let hash = req.match_info().get("hash").unwrap_or("").to_string();
    let file = req.match_info().get("file").unwrap_or("").to_string();
    if hash.len() != 32 || !hash.chars().all(|c| c.is_ascii_hexdigit()) || file != format!("{}.ogg", hash) {
        return HttpResponse::NotFound().finish();
    }
    let Some(relative) = database::find_voice_by_md5(&hash) else {
        return HttpResponse::NotFound().finish();
    };
    match fs::read(asset_path(&relative)) {
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

// The per-file cap is enforced while the field is still streaming, BEFORE any
// byte reaches the png decoder, so a decompression bomb is never decoded. The
// per-request cap is checked over the running total
async fn read_multipart(mut payload: Multipart) -> Result<Fields, String> {
    let mut fields = Fields::new();
    let mut total = 0usize;
    while let Some(mut field) = payload.try_next().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();
        let mut data = Vec::new();
        while let Some(chunk) = field.try_next().await.map_err(|e| e.to_string())? {
            total += chunk.len();
            if total > MAX_REQUEST_BYTES {
                return Err(format!("Upload exceeds the {} MB per-request limit", MAX_REQUEST_BYTES / (1024 * 1024)));
            }
            data.extend_from_slice(&chunk);
            if data.len() > MAX_FILE_BYTES {
                return Err(format!("'{}' exceeds the {} MB per-file limit", name, MAX_FILE_BYTES / (1024 * 1024)));
            }
        }
        fields.insert(name, data);
    }
    Ok(fields)
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

// Masterdata writes level-indexed skill arrays slash-separated ("25/24/24");
// an HTML form is more naturally comma-separated. Both are accepted
fn parse_levels(raw: &str, label: &str) -> Result<Vec<i64>, String> {
    let mut rv = Vec::new();
    for part in raw.split(['/', ',']) {
        let part = part.trim();
        if part.is_empty() {
            continue;
        }
        let value = part.parse::<i64>().map_err(|_| format!("{}: '{}' is not a number", label, part))?;
        if value < 0 || value > u32::MAX as i64 {
            return Err(format!("{}: '{}' does not fit in a uint", label, part));
        }
        rv.push(value);
    }
    Ok(rv)
}

fn levels_of(fields: &Fields, key: &str, stored: &JsonValue, stored_key: &str) -> Result<Vec<i64>, String> {
    if fields.contains_key(key) {
        return parse_levels(&field_str(fields, key), key);
    }
    Ok(stored[stored_key].members().filter_map(|v| v.as_i64()).collect())
}

fn to_json_array(values: &[i64]) -> JsonValue {
    let mut rv = array![];
    for value in values {
        rv.push(*value).unwrap();
    }
    rv
}

// illust_id is {prefix:05}_{seq:04}_{00|01}, derived from the id and never
// uploaded
fn illust_id(master_card_id: i64, variant: &str) -> String {
    format!("{:05}_{:04}_{}", master_card_id / 10000, master_card_id % 10000, variant)
}

struct PendingArt {
    file: String,
    entry: JsonValue,
    bytes: Vec<u8>
}

fn pending(kind: &str, variant: Option<&str>, png: Vec<u8>) -> PendingArt {
    let name = match variant {
        Some(variant) => format!("{}_{}", kind, variant),
        None => kind.to_string()
    };
    let mut entry = object!{
        "kind": kind,
        "md5": format!("{:x}", md5::compute(&png)),
        "size": png.len()
    };
    if let Some(variant) = variant {
        entry["variant"] = variant.into();
    }
    PendingArt {
        file: format!("{}.png", name),
        entry,
        bytes: png
    }
}

fn oversized(name: &str, bytes: &[u8]) -> Result<(), String> {
    if bytes.len() > MAX_FILE_BYTES {
        return Err(format!("'{}' exceeds the {} MB per-file limit", name, MAX_FILE_BYTES / (1024 * 1024)));
    }
    Ok(())
}

// An explicit per-kind override: any decodable format, any dimensions -
// cover-cropped to the target aspect and resized, then stored as PNG
fn process_override(name: &str, bytes: &[u8], kind: &ArtKind) -> Result<Vec<u8>, String> {
    oversized(name, bytes)?;
    let img = art::decode_source(name, bytes)?;
    art::encode_png(&art::cover(&img.to_rgba8(), kind.width, kind.height))
}

// Card art per variant: the source artwork (art_00 / art_01) derives all 7
// kinds via the import pipeline's recipes; explicit per-kind files override
// the derived ones. On create both sources are required; on update an absent
// source keeps the stored art (individual overrides still replace their kind)
fn collect_card_art(fields: &Fields, require_sources: bool) -> Result<Vec<PendingArt>, String> {
    let mut rv = Vec::new();
    for variant in CARD_ART_VARIANTS {
        let source_name = format!("art_{}", variant);
        let mut derived: HashMap<&'static str, image::RgbaImage> = HashMap::new();
        if let Some(bytes) = file_of(fields, &source_name) {
            oversized(&source_name, bytes)?;
            let img = art::decode_source(&source_name, bytes)?;
            derived = art::derive_card_art(&img).into_iter().collect();
        } else if require_sources {
            let label = if *variant == "00" { "normal" } else { "evolved" };
            return Err(format!("'{}' ({} card artwork) is required", source_name, label));
        }
        for kind in CARD_ART {
            let name = format!("{}_{}", kind.kind, variant);
            let png = if let Some(bytes) = file_of(fields, &name) {
                Some(process_override(&name, bytes, kind)?)
            } else if let Some(img) = derived.remove(kind.kind) {
                Some(art::encode_png(&img)?)
            } else {
                None
            };
            if let Some(png) = png {
                rv.push(pending(kind.kind, Some(variant), png));
            }
        }
    }
    Ok(rv)
}

// Character art: the portrait (pr), signature (sign) and standing art
// (character) are distinct content and stay separate inputs; the icon is
// derived from the portrait unless explicitly supplied. Everything is
// cover-cropped to target, never rejected for dimensions
fn collect_character_art(fields: &Fields, require_all: bool) -> Result<Vec<PendingArt>, String> {
    let mut rv = Vec::new();
    let mut portrait: Option<image::DynamicImage> = None;
    for kind in CHARACTER_ART {
        let png = if let Some(bytes) = file_of(fields, kind.kind) {
            oversized(kind.kind, bytes)?;
            let img = art::decode_source(kind.kind, bytes)?;
            let png = art::encode_png(&art::cover(&img.to_rgba8(), kind.width, kind.height))?;
            if kind.kind == "pr" {
                portrait = Some(img);
            }
            Some(png)
        } else if kind.kind == "icon" {
            // Derived from the portrait; on an update with no new portrait
            // the stored icon stays
            portrait.as_ref().map(|img| art::encode_png(&art::derive_character_icon(img))).transpose()?
        } else if require_all {
            return Err(format!("'{}' art is required", kind.kind));
        } else {
            None
        };
        if let Some(png) = png {
            rv.push(pending(kind.kind, None, png));
        }
    }
    Ok(rv)
}

// The catalog's art list: the stored entries with every replaced file swapped
// out. The (kind, variant) pair is the client's cache key and md5 is the hash
// of the exact bytes the data route serves
fn merge_art(stored: &JsonValue, pending: &[PendingArt]) -> JsonValue {
    let mut rv = array![];
    for art in stored.members() {
        if pending.iter().any(|new| new.entry["kind"] == art["kind"] && new.entry["variant"] == art["variant"]) {
            continue;
        }
        rv.push(art.clone()).unwrap();
    }
    for art in pending {
        rv.push(art.entry.clone()).unwrap();
    }
    rv
}

fn write_art(dir: &str, pending: &[PendingArt]) -> Result<(), String> {
    fs::create_dir_all(dir).map_err(|e| e.to_string())?;
    for art in pending {
        fs::write(format!("{}/{}", dir, art.file), &art.bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// The character's voiceline set after applying this form: stored lines kept
// unless deleted, new files decoded + transcoded to ogg-vorbis, captions
// updated in place, and each kind's survivors renumbered 1..n. Returns the
// wire-shape array and the new (md5, ogg bytes) files to write
fn collect_voice(fields: &Fields, stored_voice: &JsonValue) -> Result<(JsonValue, Vec<(String, Vec<u8>)>), String> {
    let mut rv = array![];
    let mut files: Vec<(String, Vec<u8>)> = Vec::new();
    for kind in VOICE_KINDS {
        let mut lines: Vec<JsonValue> = Vec::new();
        for index in 1..=MAX_VOICE_VARIANTS {
            let base = format!("voice_{}_{}", kind, index);
            let stored_line = stored_voice.members().find(|line| line["kind"] == *kind && line["index"] == index);
            if field_flag(fields, &format!("{}_delete", base)) {
                if file_of(fields, &base).is_some() {
                    return Err(format!("'{}': cannot both replace and delete the same line", base));
                }
                continue;
            }
            let text = |suffix: &str| -> String {
                let key = format!("{}_{}", base, suffix);
                if fields.contains_key(&key) {
                    field_str(fields, &key)
                } else {
                    stored_line.map(|line| line[suffix].as_str().unwrap_or("").to_string()).unwrap_or_default()
                }
            };
            if let Some(bytes) = file_of(fields, &base) {
                if bytes.len() > MAX_VOICE_BYTES {
                    return Err(format!("'{}' exceeds the {} MB per-file limit for voicelines", base, MAX_VOICE_BYTES / (1024 * 1024)));
                }
                let clip = audio::process_one_shot(bytes, MAX_VOICE_SECONDS).map_err(|e| format!("'{}': {}", base, e))?;
                lines.push(object!{
                    "kind": *kind,
                    "index": 0,
                    "md5": clip.md5.clone(),
                    "size": clip.bytes.len(),
                    "text": text("text"),
                    "text_en": text("text_en")
                });
                files.push((clip.md5, clip.bytes));
            } else if let Some(stored_line) = stored_line {
                lines.push(object!{
                    "kind": *kind,
                    "index": 0,
                    "md5": stored_line["md5"].clone(),
                    "size": stored_line["size"].clone(),
                    "text": text("text"),
                    "text_en": text("text_en")
                });
            }
            // Caption fields for a slot with neither a file nor a stored line
            // are ignored, like unknown multipart fields everywhere else
        }
        for (i, mut line) in lines.into_iter().enumerate() {
            line["index"] = (i as i64 + 1).into();
            rv.push(line).unwrap();
        }
    }
    Ok((rv, files))
}

fn voice_path(master_character_id: i64, md5: &str) -> String {
    format!("{}/voice/{}.ogg", character_dir(master_character_id), md5)
}

fn write_voice(master_character_id: i64, files: &[(String, Vec<u8>)]) -> Result<(), String> {
    if files.is_empty() {
        return Ok(());
    }
    fs::create_dir_all(format!("{}/voice", character_dir(master_character_id))).map_err(|e| e.to_string())?;
    for (md5, bytes) in files {
        fs::write(voice_path(master_character_id, md5), bytes).map_err(|e| e.to_string())?;
    }
    Ok(())
}

// Files whose md5 no longer appears in the voice array are gone for good
// (their old catalog md5 404s, exactly like replaced art)
fn gc_voice(master_character_id: i64, old_voice: &JsonValue, new_voice: &JsonValue) {
    for old in old_voice.members() {
        let md5 = old["md5"].to_string();
        if !md5.is_empty() && !new_voice.members().any(|line| line["md5"] == old["md5"]) {
            let _ = fs::remove_file(voice_path(master_character_id, &md5));
        }
    }
}

// card.upload manages your own uploads, card.edit is moderation over anybody's
fn can_manage(uid: i64, owner: i64) -> bool {
    permissions::has(uid, permissions::CARD_EDIT)
        || (owner == uid && permissions::has(uid, permissions::CARD_UPLOAD))
}

fn can_publish(uid: i64, owner: i64) -> bool {
    permissions::has(uid, permissions::CARD_EDIT)
        || (owner == uid && permissions::has(uid, permissions::CARD_PUBLISH))
}

// A card may reference an official/imported character, or a custom one the
// uploader owns (or that is already publicly visible through a published card)
fn validate_character_ref(uid: i64, master_character_id: i64) -> Result<(), String> {
    if OFFICIAL_CHARACTER_IDS.contains(&master_character_id) {
        return Ok(());
    }
    if database::has_character(master_character_id)
        && (database::get_character_owner(master_character_id) == Some(uid)
            || database::character_publicly_visible(master_character_id)
            || permissions::has(uid, permissions::CARD_EDIT)) {
        return Ok(());
    }
    Err(format!("Unknown master_character_id '{}'", master_character_id))
}

// Every referential and range check the client cannot survive being wrong
// about. Returns the catalog blob, ready to store and serve verbatim
fn build_card(master_card_id: i64, master_character_id: i64, fields: &Fields, stored: &JsonValue) -> Result<JsonValue, String> {
    for (key, label) in [("name", "Card name"), ("name_en", "Card English name")] {
        if text_of(fields, key, stored, key).is_empty() {
            return Err(format!("{} is required", label));
        }
    }

    let card_type = number_of(fields, "type", stored, "type");
    if !(CARD_TYPE_MIN..=CARD_TYPE_MAX).contains(&card_type) {
        return Err(String::from("type must be 1-4 (1 Smile / 2 Pure / 3 Cool / 4 All)"));
    }

    let rarity = number_of(fields, "rarity", stored, "rarity");
    if !(CARD_RARITY_MIN..=CARD_RARITY_MAX).contains(&rarity) {
        return Err(String::from("rarity must be 1-3 (1 R / 2 SR / 3 UR)"));
    }
    let rarity_name = RARITY_NAMES[(rarity - 1) as usize];

    // 0 is never a valid skill_center row: the client dereferences the mst
    // with no null filter
    let master_skill_center_id = number_of(fields, "master_skill_center_id", stored, "master_skill_center_id");
    if !SKILL_CENTER_IDS.contains(&master_skill_center_id) {
        return Err(format!("Unknown master_skill_center_id '{}'", master_skill_center_id));
    }

    // Note the official HP scale before assuming a bug report: hp is a tiny
    // per-rarity constant in SIF2 (every official R card has 2, SR 3, UR 4)
    let caps = STAT_CAPS.get(&rarity).copied().unwrap_or((0, 0, 0, 0));
    let stats = [("hp", caps.0), ("smile", caps.1), ("cool", caps.2), ("pure", caps.3)];
    let mut values = Vec::new();
    for (key, cap) in stats {
        let value = number_of(fields, key, stored, key);
        if !(1..=cap).contains(&value) {
            return Err(format!("{} must be between 1 and {} for a {} card (the official {} range)", key, cap, rarity_name, rarity_name));
        }
        values.push(value);
    }

    let levels = *SKILL_LEVEL_COUNT.get(&rarity).unwrap_or(&0);
    let stored_skill = stored["skill"].clone();

    for (key, label) in [
        ("skill_name", "Skill name"),
        ("skill_name_en", "Skill English name"),
        ("skill_detail_text", "Skill description"),
        ("skill_detail_text_en", "Skill English description")
    ] {
        if text_of(fields, key, &stored_skill, &key["skill_".len()..]).is_empty() {
            return Err(format!("{} is required", label));
        }
    }

    let trigger = number_of(fields, "skill_trigger", &stored_skill, "trigger");
    if !(SKILL_TRIGGER_MIN..=SKILL_TRIGGER_MAX).contains(&trigger) {
        return Err(String::from("skill_trigger must be 1-4 (1 rhythm icons / 2 combo / 3 PERFECTs / 4 seconds)"));
    }
    let effect_type = number_of(fields, "skill_effect_type", &stored_skill, "effect_type");
    if !(SKILL_EFFECT_TYPE_MIN..=SKILL_EFFECT_TYPE_MAX).contains(&effect_type) {
        return Err(String::from("skill_effect_type must be 1-11 (1-3 stat up / 4 score / 5 perfect window / 6 heal / 7 skill chance / 8 skill boost / 9 param sync / 10 combo fever / 11 skill repeat)"));
    }
    let sub_target = number_of(fields, "skill_sub_target", &stored_skill, "sub_target");
    if !(0..=SKILL_SUB_TARGET_MAX).contains(&sub_target) {
        return Err(String::from("skill_sub_target must be 0 or 1 (0 every rhythm icon / 1 PERFECT icons only)"));
    }
    let target_group_id = number_of(fields, "skill_target_group_id", &stored_skill, "target_group_id");
    if target_group_id != 0 && !GROUP_IDS.contains(&target_group_id) {
        return Err(format!("skill_target_group_id must be 0 or a real group id, got '{}'", target_group_id));
    }
    let target_school_grade = number_of(fields, "skill_target_school_grade", &stored_skill, "target_school_grade");
    if !(0..=SKILL_SCHOOL_GRADE_MAX).contains(&target_school_grade) {
        return Err(String::from("skill_target_school_grade must be 0-3 (0 = no grade filter)"));
    }

    // The client walks these in lockstep with the skill level, so each must
    // carry exactly one value per level of the rarity's curve. Shipped
    // masterdata also allows a single constant duration, so
    // effective_milli_secs may be 1 long
    let mut arrays: HashMap<&str, Vec<i64>> = HashMap::new();
    for (form, key, max) in [
        ("skill_trigger_value", "trigger_value", u32::MAX as i64),
        ("skill_probability", "probability", SKILL_PROBABILITY_MAX),
        ("skill_effective_milli_secs", "effective_milli_secs", SKILL_MILLI_SECS_MAX),
        ("skill_effective_values", "effective_values", u32::MAX as i64)
    ] {
        let values = levels_of(fields, form, &stored_skill, key)?;
        if values.len() != levels && !(key == "effective_milli_secs" && values.len() == 1) {
            return Err(format!("{} needs exactly {} values for a rarity {} card, got {}", form, levels, rarity, values.len()));
        }
        if let Some(value) = values.iter().find(|v| **v > max) {
            return Err(format!("{}: '{}' exceeds the maximum of {}", form, value, max));
        }
        arrays.insert(key, values);
    }

    Ok(object!{
        "master_card_id": master_card_id,
        "master_character_id": master_character_id,
        "name": text_of(fields, "name", stored, "name"),
        "name_en": text_of(fields, "name_en", stored, "name_en"),
        "type": card_type,
        "rarity": rarity,
        "master_skill_center_id": master_skill_center_id,
        "master_skill_id": master_card_id,
        "hp": values[0],
        "smile": values[1],
        "cool": values[2],
        "pure": values[3],
        "illust_id": illust_id(master_card_id, "00"),
        "evolve_illust_id": illust_id(master_card_id, "01"),
        // Official cards use the level curve matching their rarity (1/2/3),
        // verified across every row of card.csv
        "master_card_level_id": rarity,
        "unique_background_file_name": "",
        "evolve_unique_background_file_name": "",
        "get_category": GET_CATEGORY_GACHA,
        "master_card_sys_voice_id": 0,
        "album_unit_m_id": 0,
        "priority": 0,
        "master_release_label_id": MASTER_RELEASE_LABEL_ID,
        "skill": {
            "name": text_of(fields, "skill_name", &stored_skill, "name"),
            "name_en": text_of(fields, "skill_name_en", &stored_skill, "name_en"),
            "detail_text": text_of(fields, "skill_detail_text", &stored_skill, "detail_text"),
            "detail_text_en": text_of(fields, "skill_detail_text_en", &stored_skill, "detail_text_en"),
            "trigger": trigger,
            "trigger_value": to_json_array(&arrays["trigger_value"]),
            "probability": to_json_array(&arrays["probability"]),
            "effective_milli_secs": to_json_array(&arrays["effective_milli_secs"]),
            "sub_target": sub_target,
            "target_group_id": target_group_id,
            "target_school_grade": target_school_grade,
            "effect_type": effect_type,
            "effective_values": to_json_array(&arrays["effective_values"])
        },
        "art": JsonValue::Null // filled by the caller with merge_art
    })
}

fn valid_color(color: &str) -> bool {
    color.len() == 6 && color.chars().all(|c| c.is_ascii_hexdigit())
}

// Every column the uploader never supplies is forced to the value all 172
// imported characters carry. Numbers, not enum names
fn build_character(master_character_id: i64, fields: &Fields, stored: &JsonValue) -> Result<JsonValue, String> {
    for (key, label) in [
        ("character_name", "Character name"),
        ("character_name_en", "Character English name"),
        ("character_name_ruby", "Character name reading"),
        ("character_name_ruby_en", "Character English name reading"),
        ("character_detail_text", "Character description"),
        ("character_detail_text_en", "Character English description"),
        ("character_name_richtext_gacha", "Character gacha display name"),
        ("character_name_richtext_gacha_en", "Character English gacha display name")
    ] {
        if text_of(fields, key, stored, &key["character_".len()..]).is_empty() {
            return Err(format!("{} is required", label));
        }
    }
    for key in ["character_image_color", "character_image_color_dark"] {
        if !valid_color(&text_of(fields, key, stored, &key["character_".len()..])) {
            return Err(format!("{} must be a 6-digit hex color like FF9210", key));
        }
    }
    let text = |key: &str| text_of(fields, &format!("character_{}", key), stored, key);
    Ok(object!{
        "master_character_id": master_character_id,
        "name": text("name"),
        "name_en": text("name_en"),
        "name_ruby": text("name_ruby"),
        "name_ruby_en": text("name_ruby_en"),
        "detail_text": text("detail_text"),
        "detail_text_en": text("detail_text_en"),
        "category": CHARACTER_CATEGORY_OTHER,
        "school_grade": CHARACTER_SCHOOL_GRADE,
        "chara_category": CHARACTER_CHARA_CATEGORY,
        "master_group_id": CHARACTER_GROUP_ID,
        "sprite_name": "",
        "display_order": master_character_id,
        "height": text("height"),
        "blood_type": text("blood_type"),
        "blood_type_en": text("blood_type_en"),
        "birthday": text("birthday"),
        "birthday_en": text("birthday_en"),
        "voice_actor": text("voice_actor"),
        "voice_actor_en": text("voice_actor_en"),
        "image_color": text("image_color"),
        "image_color_dark": text("image_color_dark"),
        "name_richtext_gacha": text("name_richtext_gacha"),
        "name_richtext_gacha_en": text("name_richtext_gacha_en"),
        "master_release_label_id": MASTER_RELEASE_LABEL_ID,
        "art": JsonValue::Null // filled by the caller with merge_art
    })
}

pub fn create_card(uid: i64, fields: &Fields) -> Result<i64, String> {
    if !permissions::has(uid, permissions::CARD_UPLOAD) {
        return Err(String::from("You do not have permission to upload cards"));
    }
    if database::card_count_for_owner(uid) >= MAX_CARDS_PER_USER {
        return Err(format!("You have reached the {} card limit", MAX_CARDS_PER_USER));
    }
    let published = field_flag(fields, "published");
    let obtainable = field_flag(fields, "obtainable");
    if (published || obtainable) && !can_publish(uid, uid) {
        return Err(String::from("You do not have permission to publish cards"));
    }

    let master_character_id = field_str(fields, "master_character_id").parse::<i64>().unwrap_or(0);
    validate_character_ref(uid, master_character_id)?;

    // Fail fast: every cheap field/enum/range/reference check runs (as a
    // dry-run against a placeholder id) BEFORE any image is decoded or
    // derived, so a form mistake rejects instantly instead of after seconds
    // of art processing
    build_card(0, master_character_id, fields, &object!{})?;

    let card_art = collect_card_art(fields, true)?;

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let master_card_id = database::next_card_id();
    if master_card_id > database::LAST_CARD_ID {
        return Err(String::from("The custom card id space is exhausted"));
    }
    if !databases::CARD_LIST[master_card_id.to_string()].is_empty() {
        return Err(format!("Card id {} already exists in masterdata", master_card_id));
    }

    let mut card = build_card(master_card_id, master_character_id, fields, &object!{})?;
    card["art"] = merge_art(&array![], &card_art);

    write_art(&card_dir(master_card_id), &card_art)?;
    database::insert_card(master_card_id, master_character_id, uid, &card, published, obtainable);
    database::bump_revision();
    drop(lock);

    Ok(master_card_id)
}

// Edit a card in place. The master_card_id - and everything derived from it:
// master_skill_id, illust ids - stays the same, so a player who owns the card
// keeps owning the same card. master_character_id is fixed too: repointing it
// would orphan a custom character mid-catalog
pub fn update_card(uid: i64, master_card_id: i64, fields: &Fields) -> Result<(), String> {
    let Some(owner) = database::get_card_owner(master_card_id) else {
        return Err(String::from("Card not found"));
    };
    if !can_manage(uid, owner) {
        return Err(String::from("You do not have permission to edit this card"));
    }
    let stored = database::get_card(master_card_id).ok_or(String::from("Card not found"))?;
    let master_character_id = stored["master_character_id"].as_i64().unwrap_or(0);

    // Field validation first (cheap), art processing second - fail fast
    let mut card = build_card(master_card_id, master_character_id, fields, &stored)?;
    let card_art = collect_card_art(fields, false)?;
    card["art"] = merge_art(&stored["art"], &card_art);

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    write_art(&card_dir(master_card_id), &card_art)?;
    database::update_card(master_card_id, &card);
    database::bump_revision();
    drop(lock);

    Ok(())
}

// Publish/unpublish and the obtainable toggle share a route: both are
// catalog-flag flips on an owned card
pub fn set_card_flags(uid: i64, master_card_id: i64, published: Option<bool>, obtainable: Option<bool>) -> Result<(), String> {
    let Some(owner) = database::get_card_owner(master_card_id) else {
        return Err(String::from("Card not found"));
    };
    if !can_publish(uid, owner) {
        return Err(String::from("You do not have permission to publish this card"));
    }
    if let Some(published) = published {
        database::set_published(master_card_id, published);
    }
    if let Some(obtainable) = obtainable {
        database::set_obtainable(master_card_id, obtainable);
    }
    database::bump_revision();
    Ok(())
}

// Deleting retires the id forever. Player copies of the dead card are wiped
// lazily on each account's next userdata pull (userdata::remove_deleted_
// custom_cards), mirroring how deleted custom songs clean up. The character
// stays: it may back other cards, and has its own delete route
pub fn delete_card(uid: i64, master_card_id: i64) -> Result<(), String> {
    let Some(owner) = database::get_card_owner(master_card_id) else {
        return Err(String::from("Card not found"));
    };
    if !can_manage(uid, owner) {
        return Err(String::from("You do not have permission to delete this card"));
    }
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    database::delete_card(master_card_id);
    database::bump_revision();
    drop(lock);
    let _ = fs::remove_dir_all(card_dir(master_card_id));
    Ok(())
}

pub fn create_character(uid: i64, fields: &Fields) -> Result<i64, String> {
    if !permissions::has(uid, permissions::CARD_UPLOAD) {
        return Err(String::from("You do not have permission to upload characters"));
    }
    // Fail fast: cheap text validation before any image/audio work
    build_character(0, fields, &object!{})?;
    let character_art = collect_character_art(fields, true)?;
    let (voice, voice_files) = collect_voice(fields, &array![])?;

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    let master_character_id = database::next_character_id();
    if master_character_id > database::LAST_CHARACTER_ID {
        return Err(String::from("The custom character id space is exhausted"));
    }
    if OFFICIAL_CHARACTER_IDS.contains(&master_character_id) {
        return Err(format!("Character id {} already exists in masterdata", master_character_id));
    }

    let mut character = build_character(master_character_id, fields, &object!{})?;
    character["art"] = merge_art(&array![], &character_art);
    if !voice.is_empty() {
        character["voice"] = voice;
    }

    write_art(&character_dir(master_character_id), &character_art)?;
    write_voice(master_character_id, &voice_files)?;
    database::insert_character(master_character_id, uid, &character);
    database::bump_revision();
    drop(lock);

    Ok(master_character_id)
}

pub fn update_character(uid: i64, master_character_id: i64, fields: &Fields) -> Result<(), String> {
    let Some(owner) = database::get_character_owner(master_character_id) else {
        return Err(String::from("Character not found"));
    };
    if !can_manage(uid, owner) {
        return Err(String::from("You do not have permission to edit this character"));
    }
    let stored = database::get_character(master_character_id).ok_or(String::from("Character not found"))?;

    // Field validation first (cheap), image/audio processing second - fail fast
    let mut character = build_character(master_character_id, fields, &stored)?;
    let character_art = collect_character_art(fields, false)?;
    let (voice, voice_files) = collect_voice(fields, &stored["voice"])?;
    character["art"] = merge_art(&stored["art"], &character_art);
    if !voice.is_empty() {
        character["voice"] = voice.clone();
    }

    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    write_art(&character_dir(master_character_id), &character_art)?;
    write_voice(master_character_id, &voice_files)?;
    database::update_character(master_character_id, &character);
    database::bump_revision();
    drop(lock);

    // Replaced/deleted lines: their oggs are per-character, so a md5 gone
    // from the blob is safe to remove
    gc_voice(master_character_id, &stored["voice"], &voice);

    Ok(())
}

// A character can only go once nothing references it - a dangling
// master_character_id in a served card is a client crash
pub fn delete_character(uid: i64, master_character_id: i64) -> Result<(), String> {
    let Some(owner) = database::get_character_owner(master_character_id) else {
        return Err(String::from("Character not found"));
    };
    if !can_manage(uid, owner) {
        return Err(String::from("You do not have permission to delete this character"));
    }
    let referenced = database::cards_using_character(master_character_id);
    if referenced > 0 {
        return Err(format!("{} card(s) still use this character - delete them first", referenced));
    }
    let lock = lock_onto_mutex!(UPLOAD_LOCK);
    database::delete_character(master_character_id);
    database::bump_revision();
    drop(lock);
    let _ = fs::remove_dir_all(character_dir(master_character_id));
    Ok(())
}

async fn create(req: HttpRequest, payload: Multipart) -> HttpResponse {
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
    match create_card(uid, &fields) {
        Ok(master_card_id) => send_json(object!{
            result: "OK",
            master_card_id: master_card_id
        }),
        Err(e) => webui::error(&e)
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
    let master_card_id = field_str(&fields, "master_card_id").parse::<i64>().unwrap_or(0);
    match update_card(uid, master_card_id, &fields) {
        Ok(()) => send_json(object!{
            result: "OK",
            master_card_id: master_card_id
        }),
        Err(e) => webui::error(&e)
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
    let master_card_id = body["master_card_id"].as_i64().unwrap_or(0);
    match set_card_flags(uid, master_card_id, body["published"].as_bool(), body["obtainable"].as_bool()) {
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
    match delete_card(uid, body["master_card_id"].as_i64().unwrap_or(0)) {
        Ok(()) => send_json(object!{
            result: "OK"
        }),
        Err(e) => webui::error(&e)
    }
}

async fn character_create(req: HttpRequest, payload: Multipart) -> HttpResponse {
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
    match create_character(uid, &fields) {
        Ok(master_character_id) => send_json(object!{
            result: "OK",
            master_character_id: master_character_id
        }),
        Err(e) => webui::error(&e)
    }
}

async fn character_update(req: HttpRequest, payload: Multipart) -> HttpResponse {
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
    let master_character_id = field_str(&fields, "master_character_id").parse::<i64>().unwrap_or(0);
    match update_character(uid, master_character_id, &fields) {
        Ok(()) => send_json(object!{
            result: "OK",
            master_character_id: master_character_id
        }),
        Err(e) => webui::error(&e)
    }
}

async fn character_delete(req: HttpRequest, body: String) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let Some(uid) = get_session_uid(&req) else {
        return webui::error("Not logged in");
    };
    let body = jzon::parse(&body).unwrap_or(object!{});
    match delete_character(uid, body["master_character_id"].as_i64().unwrap_or(0)) {
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
        cards: database::get_cards_by_owner(uid),
        characters: database::get_characters_by_owner(uid)
    })
}

// The public card browser: the published catalog with uploader names, plus
// the custom characters those cards reference. Anonymous viewers are fine -
// published means public
async fn browse(_req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    let mut cards = database::get_browse_cards();
    for card in cards.members_mut() {
        card["uploader"] = userdata::get_name_and_rank(card["owner_id"].as_i64().unwrap_or(0))["user_name"].clone();
        card.remove("owner_id");
    }
    let characters = database::get_characters_for_cards(0, &cards);
    send_json(object!{
        result: "OK",
        cards: cards,
        characters: characters
    })
}

#[cfg(test)]
pub mod tests {
    use super::*;

    // Distinct bytes per file, so every art entry gets its own md5 and a
    // content-addressed lookup can be asserted per kind
    pub fn seeded_png(width: u32, height: u32, seed: u8) -> Vec<u8> {
        let mut rv = Vec::new();
        image::DynamicImage::ImageRgba8(image::RgbaImage::from_fn(width, height, |x, y| {
            image::Rgba([(x % 256) as u8, (y % 256) as u8, seed, 255])
        })).write_to(&mut std::io::Cursor::new(&mut rv), image::ImageFormat::Png).unwrap();
        rv
    }

    // A navi-style source: transparent background, opaque figure - exercises
    // the cutout derivation lane. Deliberately odd-sized: every derived kind
    // must still come out at the exact official target size
    pub fn cutout_png(width: u32, height: u32, seed: u8) -> Vec<u8> {
        let mut img = image::RgbaImage::from_pixel(width, height, image::Rgba([0, 0, 0, 0]));
        for y in height / 8..height * 7 / 8 {
            for x in width / 3..width * 2 / 3 {
                img.put_pixel(x, y, image::Rgba([(x % 256) as u8, (y % 256) as u8, seed, 255]));
            }
        }
        let mut rv = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut rv), image::ImageFormat::Png).unwrap();
        rv
    }

    pub fn field(fields: &mut Fields, key: &str, value: &str) {
        fields.insert(String::from(key), value.as_bytes().to_vec());
    }

    // A complete, valid rarity-1 card upload
    pub fn base_fields() -> Fields {
        let mut fields = Fields::new();
        field(&mut fields, "name", "Test Card");
        field(&mut fields, "name_en", "Test Card EN");
        field(&mut fields, "master_character_id", "1001");
        field(&mut fields, "type", "1");
        field(&mut fields, "rarity", "1");
        field(&mut fields, "hp", "2");
        field(&mut fields, "smile", "1000");
        field(&mut fields, "cool", "1000");
        field(&mut fields, "pure", "1000");
        field(&mut fields, "master_skill_center_id", "100001");
        field(&mut fields, "skill_name", "Test Skill");
        field(&mut fields, "skill_name_en", "Test Skill EN");
        field(&mut fields, "skill_detail_text", "Does a thing");
        field(&mut fields, "skill_detail_text_en", "Does a thing in English");
        field(&mut fields, "skill_trigger", "3");
        field(&mut fields, "skill_trigger_value", "25/24/24");
        field(&mut fields, "skill_probability", "39000/39000/39000");
        field(&mut fields, "skill_effective_milli_secs", "2000/2000/2000");
        field(&mut fields, "skill_sub_target", "0");
        field(&mut fields, "skill_target_group_id", "0");
        field(&mut fields, "skill_target_school_grade", "0");
        field(&mut fields, "skill_effect_type", "4");
        field(&mut fields, "skill_effective_values", "124/126/128");
        // Odd-sized sources: the server derives all 7 kinds per variant
        fields.insert(String::from("art_00"), cutout_png(870, 1100, 10));
        fields.insert(String::from("art_01"), seeded_png(1333, 987, 20));
        fields
    }

    pub fn character_fields() -> Fields {
        let mut fields = Fields::new();
        field(&mut fields, "character_name", "Test Chara");
        field(&mut fields, "character_name_en", "Test Chara EN");
        field(&mut fields, "character_name_ruby", "てすと");
        field(&mut fields, "character_name_ruby_en", "Tesuto");
        field(&mut fields, "character_detail_text", "A test character.");
        field(&mut fields, "character_detail_text_en", "A test character (EN).");
        field(&mut fields, "character_name_richtext_gacha", "Test Chara");
        field(&mut fields, "character_name_richtext_gacha_en", "Test Chara");
        field(&mut fields, "character_height", "");
        field(&mut fields, "character_blood_type", "");
        field(&mut fields, "character_blood_type_en", "");
        field(&mut fields, "character_birthday", "？月？日");
        field(&mut fields, "character_birthday_en", "");
        field(&mut fields, "character_voice_actor", "");
        field(&mut fields, "character_voice_actor_en", "");
        field(&mut fields, "character_image_color", "888888");
        field(&mut fields, "character_image_color_dark", "888888");
        // Odd sizes on purpose; no icon - it derives from the portrait
        fields.insert(String::from("pr"), seeded_png(431, 617, 201));
        fields.insert(String::from("sign"), seeded_png(600, 500, 202));
        fields.insert(String::from("character"), seeded_png(555, 999, 203));
        fields
    }

    // Permission grants need a grantor; an owner uid is the bootstrap one.
    // Owners are cleared afterwards so unrelated tests never see them
    pub fn with_permissions<T>(uid: i64, scopes: &[&str], body: impl FnOnce() -> T) -> T {
        crate::runtime::update_owners(&[9_000_001]);
        for scope in scopes {
            permissions::grant(uid, scope, 9_000_001).unwrap();
        }
        let rv = body();
        for scope in scopes {
            let _ = permissions::revoke(uid, scope, 9_000_001);
        }
        crate::runtime::update_owners(&[]);
        rv
    }

    pub fn wipe(uid: i64) {
        crate::runtime::update_owners(&[uid]);
        for card in database::get_cards_by_owner(uid).members() {
            let _ = delete_card(uid, card["master_card_id"].as_i64().unwrap());
        }
        for character in database::get_characters_by_owner(uid).members() {
            let _ = delete_character(uid, character["master_character_id"].as_i64().unwrap());
        }
        crate::runtime::update_owners(&[]);
    }

    // Seeded 44.1kHz 16-bit mono wav, so different seeds give different md5s
    pub fn test_wav(seconds: f64, seed: u8) -> Vec<u8> {
        let sample_rate: u32 = 44100;
        let frames = (seconds * sample_rate as f64) as u32;
        let data_len = frames * 2;
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
        for i in 0..frames {
            let sample = (((i as f64 * (220.0 + seed as f64) * 2.0 * std::f64::consts::PI / sample_rate as f64).sin()) * 8000.0) as i16;
            rv.extend(sample.to_le_bytes());
        }
        rv
    }

    // Voicelines: transcode to ogg, wire shape + captions, renumbering,
    // caption-only edits, replacement GC, deletion, the caps, and the
    // content-addressed voice route index
    #[test]
    fn character_voicelines() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(4010);

        let mut fields = character_fields();
        // Sparse indexes on purpose: 1 and 5 must renumber to 1 and 2
        fields.insert(String::from("voice_live_start_1"), test_wav(1.0, 1));
        field(&mut fields, "voice_live_start_1_text", "いくよー！");
        field(&mut fields, "voice_live_start_1_text_en", "Here we go!");
        fields.insert(String::from("voice_live_start_5"), test_wav(1.0, 2));
        fields.insert(String::from("voice_skill_smile_1"), test_wav(0.5, 3));
        // Index 10 is out of range and ignored entirely
        fields.insert(String::from("voice_live_start_10"), test_wav(1.0, 4));
        let id = with_permissions(4010, &[permissions::CARD_UPLOAD], || create_character(4010, &fields).unwrap());

        let character = database::get_character(id).unwrap();
        let voice = &character["voice"];
        assert_eq!(voice.len(), 3);
        let live: Vec<&JsonValue> = voice.members().filter(|line| line["kind"] == "live_start").collect();
        assert_eq!(live.len(), 2);
        assert_eq!(live[0]["index"].as_i64(), Some(1));
        assert_eq!(live[1]["index"].as_i64(), Some(2));
        assert_eq!(live[0]["text"].as_str(), Some("いくよー！"));
        assert_eq!(live[0]["text_en"].as_str(), Some("Here we go!"));
        assert_eq!(live[1]["text"].as_str(), Some(""));
        for line in voice.members() {
            let md5 = line["md5"].to_string();
            assert_eq!(md5.len(), 32);
            let path = format!("{}/voice/{}.ogg", character_dir(id), md5);
            let bytes = fs::read(&path).unwrap();
            assert!(bytes.starts_with(b"OggS"), "transcoded to ogg");
            assert_eq!(format!("{:x}", md5::compute(&bytes)), md5);
            assert_eq!(bytes.len(), line["size"].as_usize().unwrap());
            // The voice route's index resolves the md5 to this exact file
            assert_eq!(database::find_voice_by_md5(&md5), Some(format!("characters/{}/voice/{}.ogg", id, md5)));
        }

        // Caption-only edit: md5 untouched, captions replaced
        let old_md5 = live[0]["md5"].to_string();
        let mut edit = Fields::new();
        field(&mut edit, "voice_live_start_1_text_en", "Let's gooo!");
        with_permissions(4010, &[permissions::CARD_UPLOAD], || update_character(4010, id, &edit).unwrap());
        let line = database::get_character(id).unwrap()["voice"].members()
            .find(|line| line["kind"] == "live_start" && line["index"] == 1).unwrap().clone();
        assert_eq!(line["md5"].to_string(), old_md5);
        assert_eq!(line["text_en"].as_str(), Some("Let's gooo!"));
        assert_eq!(line["text"].as_str(), Some("いくよー！"));

        // Replacing a line swaps the md5 and garbage-collects the old ogg
        let mut edit = Fields::new();
        edit.insert(String::from("voice_live_start_1"), test_wav(1.0, 9));
        with_permissions(4010, &[permissions::CARD_UPLOAD], || update_character(4010, id, &edit).unwrap());
        let updated = database::get_character(id).unwrap();
        let new_md5 = updated["voice"].members()
            .find(|line| line["kind"] == "live_start" && line["index"] == 1).unwrap()["md5"].to_string();
        assert_ne!(new_md5, old_md5);
        assert!(fs::read(format!("{}/voice/{}.ogg", character_dir(id), old_md5)).is_err());
        assert!(fs::read(format!("{}/voice/{}.ogg", character_dir(id), new_md5)).is_ok());
        assert_eq!(database::find_voice_by_md5(&old_md5), None);

        // Deleting line 2 removes its file and leaves a contiguous single line
        let gone_md5 = updated["voice"].members()
            .find(|line| line["kind"] == "live_start" && line["index"] == 2).unwrap()["md5"].to_string();
        let mut edit = Fields::new();
        field(&mut edit, "voice_live_start_2_delete", "1");
        with_permissions(4010, &[permissions::CARD_UPLOAD], || update_character(4010, id, &edit).unwrap());
        let after = database::get_character(id).unwrap();
        let live: Vec<JsonValue> = after["voice"].members().filter(|line| line["kind"] == "live_start").cloned().collect();
        assert_eq!(live.len(), 1);
        assert_eq!(live[0]["index"].as_i64(), Some(1));
        assert!(fs::read(format!("{}/voice/{}.ogg", character_dir(id), gone_md5)).is_err());

        // Replace + delete on the same slot is contradictory
        let mut edit = Fields::new();
        field(&mut edit, "voice_live_start_1_delete", "1");
        edit.insert(String::from("voice_live_start_1"), test_wav(1.0, 5));
        assert!(with_permissions(4010, &[permissions::CARD_UPLOAD], || update_character(4010, id, &edit))
            .unwrap_err().contains("cannot both replace and delete"));

        // The caps: over-long and undecodable clips are refused by name
        let mut edit = Fields::new();
        edit.insert(String::from("voice_result_bond_1"), test_wav(31.0, 6));
        let err = with_permissions(4010, &[permissions::CARD_UPLOAD], || update_character(4010, id, &edit)).unwrap_err();
        assert!(err.contains("voice_result_bond_1") && err.contains("maximum is 30"), "{}", err);
        let mut edit = Fields::new();
        edit.insert(String::from("voice_result_bond_1"), b"definitely not audio".to_vec());
        assert!(with_permissions(4010, &[permissions::CARD_UPLOAD], || update_character(4010, id, &edit))
            .unwrap_err().contains("Could not read audio file"));

        assert_eq!(database::find_voice_by_md5(&"0".repeat(32)), None);
        wipe(4010);
    }

    // A full create: derived ids, pinned columns, art md5s and the catalog
    // shape the client parses
    #[test]
    fn create_builds_the_catalog_entry() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(4001);

        let fields = base_fields();
        let id = with_permissions(4001, &[permissions::CARD_UPLOAD], || create_card(4001, &fields).unwrap());
        assert!(id >= database::FIRST_CARD_ID);
        assert!(id / 10000 >= database::FIRST_ILLUST_PREFIX);

        let card = database::get_card(id).unwrap();
        assert_eq!(card["master_card_id"].as_i64(), Some(id));
        assert_eq!(card["master_skill_id"].as_i64(), Some(id));
        assert_eq!(card["master_character_id"].as_i64(), Some(1001));
        assert_eq!(card["illust_id"].to_string(), format!("{:05}_{:04}_00", id / 10000, id % 10000));
        assert_eq!(card["evolve_illust_id"].to_string(), format!("{:05}_{:04}_01", id / 10000, id % 10000));
        assert_eq!(card["master_card_level_id"].as_i64(), Some(1));
        assert_eq!(card["master_release_label_id"].as_i64(), Some(1));
        assert_eq!(card["get_category"].as_i64(), Some(GET_CATEGORY_GACHA));
        assert_eq!(card["unique_background_file_name"].as_str(), Some(""));
        assert_eq!(card["skill"]["effective_values"].len(), 3);
        assert_eq!(card["skill"]["effect_type"].as_i64(), Some(4));
        // All 7 kinds, both variants
        assert_eq!(card["art"].len(), 14);
        // A draft: owner-only, not obtainable, never resolvable by viewers
        assert_eq!(card["obtainable"].as_bool(), Some(false));
        assert!(!database::is_published(id));
        assert!(!viewer_can_resolve(id, PROTOCOL_VERSION));

        // Every art entry hashes the exact processed bytes on disk, the data
        // route index resolves the md5 back to that file, and every derived
        // kind landed at its exact official size despite the odd-sized source
        for art in card["art"].members() {
            let path = format!("{}/{}_{}.png", card_dir(id), art["kind"], art["variant"]);
            let bytes = fs::read(&path).unwrap();
            assert_eq!(format!("{:x}", md5::compute(&bytes)), art["md5"].to_string());
            assert_eq!(bytes.len(), art["size"].as_usize().unwrap());
            let resolved = database::find_asset_by_md5(&art["md5"].to_string()).unwrap();
            assert_eq!(fs::read(asset_path(&resolved)).unwrap(), bytes);
            let img = image::load_from_memory(&bytes).unwrap();
            let target = CARD_ART.iter().find(|k| art["kind"] == k.kind).unwrap();
            assert_eq!((img.width(), img.height()), (target.width, target.height), "kind {}", art["kind"]);
        }

        // card_info synthesizes the csv shape for the runtime band
        let info = card_info(id);
        assert_eq!(info["rarity"].as_i64(), Some(1));
        assert_eq!(info["masterCardLevelId"].as_i64(), Some(1));
        assert_eq!(info["masterCharacterId"].as_i64(), Some(1001));
        assert_eq!(crate::router::items::get_rarity(id), 1);
        // Official rows still come from the baked table
        assert_eq!(card_info(10010001)["rarity"].as_i64(), Some(1));
        assert!(card_info(id + 5000).is_null());

        with_permissions(4001, &[permissions::CARD_PUBLISH], || set_card_flags(4001, id, Some(true), Some(true)).unwrap());
        assert!(viewer_can_resolve(id, PROTOCOL_VERSION));
        // A protocol-2 viewer can't fetch the catalog, so it can't resolve it
        assert!(!viewer_can_resolve(id, 2));
        assert_eq!(database::obtainable_card_ids(1), vec![id]);

        wipe(4001);
    }

    // A new character via its own route: pinned numeric columns, art, and the
    // reference/deletion lifecycle with a card built on it
    #[test]
    fn character_lifecycle() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(4002);

        let character_id = with_permissions(4002, &[permissions::CARD_UPLOAD], || create_character(4002, &character_fields()).unwrap());
        assert!(character_id >= database::FIRST_CHARACTER_ID);
        assert!(!OFFICIAL_CHARACTER_IDS.contains(&character_id));

        let character = database::get_character(character_id).unwrap();
        assert_eq!(character["category"].as_i64(), Some(6));
        assert_eq!(character["chara_category"].as_i64(), Some(1));
        assert_eq!(character["master_group_id"].as_i64(), Some(9000));
        assert_eq!(character["school_grade"].as_i64(), Some(0));
        assert_eq!(character["sprite_name"].as_str(), Some(""));
        assert_eq!(character["display_order"].as_i64(), Some(character_id));
        assert_eq!(character["master_release_label_id"].as_i64(), Some(1));
        // All 4 kinds exist even though only 3 inputs were supplied: the icon
        // derives from the portrait
        assert_eq!(character["art"].len(), 4);
        assert!(character["art"].members().any(|art| art["kind"] == "icon"));
        // Numbers, not enum names - a name here is a live-start softlock
        for key in ["category", "chara_category", "master_group_id", "school_grade"] {
            assert!(character[key].as_i64().is_some(), "{} is not numeric", key);
        }
        // Processed bytes are what's hashed, and every odd-sized input landed
        // at its exact official size
        for art in character["art"].members() {
            let path = format!("{}/{}.png", character_dir(character_id), art["kind"]);
            let bytes = fs::read(&path).unwrap();
            assert_eq!(format!("{:x}", md5::compute(&bytes)), art["md5"].to_string());
            let img = image::load_from_memory(&bytes).unwrap();
            let target = CHARACTER_ART.iter().find(|k| art["kind"] == k.kind).unwrap();
            assert_eq!((img.width(), img.height()), (target.width, target.height), "kind {}", art["kind"]);
        }

        // An explicit icon override beats the derived one
        let derived_icon = character["art"].members().find(|art| art["kind"] == "icon").unwrap()["md5"].to_string();
        let mut edit = Fields::new();
        edit.insert(String::from("icon"), seeded_png(300, 300, 210));
        with_permissions(4002, &[permissions::CARD_UPLOAD], || update_character(4002, character_id, &edit).unwrap());
        let updated = database::get_character(character_id).unwrap();
        let new_icon = updated["art"].members().find(|art| art["kind"] == "icon").unwrap()["md5"].to_string();
        assert_ne!(new_icon, derived_icon);
        let img = image::load_from_memory(&fs::read(format!("{}/icon.png", character_dir(character_id))).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (230, 230));

        // A card can reference the uploader's own character; the character
        // then can't be deleted until the card goes
        let mut fields = base_fields();
        field(&mut fields, "master_character_id", &character_id.to_string());
        let card_id = with_permissions(4002, &[permissions::CARD_UPLOAD], || create_card(4002, &fields).unwrap());
        let err = with_permissions(4002, &[permissions::CARD_UPLOAD], || delete_character(4002, character_id)).unwrap_err();
        assert!(err.contains("still use this character"), "{}", err);
        with_permissions(4002, &[permissions::CARD_UPLOAD], || delete_card(4002, card_id).unwrap());
        with_permissions(4002, &[permissions::CARD_UPLOAD], || delete_character(4002, character_id).unwrap());
        assert!(!database::has_character(character_id));
        assert!(fs::read_dir(character_dir(character_id)).is_err());

        // Another user can't reference a draft-only character of somebody else
        let other = character_fields();
        let other_id = with_permissions(4002, &[permissions::CARD_UPLOAD], || create_character(4002, &other).unwrap());
        let mut fields = base_fields();
        field(&mut fields, "master_character_id", &other_id.to_string());
        let err = with_permissions(4003, &[permissions::CARD_UPLOAD], || create_card(4003, &fields)).unwrap_err();
        assert!(err.contains("Unknown master_character_id"), "{}", err);

        wipe(4002);
        wipe(4003);
    }

    #[test]
    fn every_validation_rejection() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(4004);

        let run = |fields: &Fields| with_permissions(4004, &[permissions::CARD_UPLOAD], || create_card(4004, fields));
        let mutated = |key: &str, value: &str| {
            let mut fields = base_fields();
            field(&mut fields, key, value);
            fields
        };

        // Referential integrity
        assert!(run(&mutated("master_character_id", "999999")).unwrap_err().contains("Unknown master_character_id"));
        assert!(run(&mutated("master_character_id", "0")).unwrap_err().contains("Unknown master_character_id"));
        assert!(run(&mutated("master_skill_center_id", "0")).unwrap_err().contains("master_skill_center_id"));
        assert!(run(&mutated("master_skill_center_id", "424242")).unwrap_err().contains("master_skill_center_id"));
        assert!(run(&mutated("name", "")).unwrap_err().contains("Card name is required"));
        assert!(run(&mutated("name_en", "")).unwrap_err().contains("Card English name is required"));
        assert!(run(&mutated("skill_name", "")).unwrap_err().contains("Skill name is required"));
        assert!(run(&mutated("skill_detail_text_en", "")).unwrap_err().contains("Skill English description is required"));
        assert!(run(&mutated("skill_target_group_id", "123")).unwrap_err().contains("skill_target_group_id"));

        // Enum ranges - each one is a client crash, not a cosmetic error -
        // and every message states the actual allowed range
        assert!(run(&mutated("type", "0")).unwrap_err().contains("type must be 1-4"));
        assert!(run(&mutated("type", "5")).unwrap_err().contains("type must be 1-4"));
        assert!(run(&mutated("rarity", "0")).unwrap_err().contains("rarity must be 1-3"));
        assert!(run(&mutated("rarity", "4")).unwrap_err().contains("rarity must be 1-3"));
        assert!(run(&mutated("skill_trigger", "5")).unwrap_err().contains("skill_trigger must be 1-4"));
        assert!(run(&mutated("skill_trigger", "0")).unwrap_err().contains("skill_trigger must be 1-4"));
        assert!(run(&mutated("skill_effect_type", "12")).unwrap_err().contains("skill_effect_type must be 1-11"));
        assert!(run(&mutated("skill_effect_type", "0")).unwrap_err().contains("skill_effect_type must be 1-11"));
        assert!(run(&mutated("skill_sub_target", "2")).unwrap_err().contains("skill_sub_target must be 0 or 1"));
        assert!(run(&mutated("skill_target_school_grade", "4")).unwrap_err().contains("skill_target_school_grade must be 0-3"));

        // Skill array shapes: rarity 1 needs exactly 3 level entries; a lone
        // effective_milli_secs is the one shipped exception
        assert!(run(&mutated("skill_effective_values", "124/126")).unwrap_err().contains("skill_effective_values"));
        assert!(run(&mutated("skill_probability", "1/2/3/4")).unwrap_err().contains("skill_probability"));
        assert!(run(&mutated("skill_effective_values", "1/2/-3")).unwrap_err().contains("uint"));
        assert!(run(&mutated("skill_effective_values", "1/2/x")).unwrap_err().contains("not a number"));
        assert!(run(&mutated("skill_trigger_value", "")).unwrap_err().contains("skill_trigger_value"));
        assert!(run(&mutated("skill_effective_milli_secs", "2000")).is_ok());
        assert!(run(&mutated("skill_probability", "2000000/1/1")).unwrap_err().contains("maximum"));
        // Rarity 2 wants 5 entries, so the rarity-1 arrays no longer fit
        assert!(run(&mutated("rarity", "2")).unwrap_err().contains("needs exactly 5"));

        // Stat bounds, computed from the official card.csv: hp really is a
        // tiny per-rarity constant (R 2 / SR 3 / UR 4), so an R card allows
        // 1-2 - and the message says so instead of hiding the limit
        let caps = STAT_CAPS.get(&1).copied().unwrap();
        assert_eq!(caps.0, 2, "official R hp cap");
        assert!(caps.1 > 0);
        let smile_err = run(&mutated("smile", &(caps.1 + 1).to_string())).unwrap_err();
        assert!(smile_err.contains(&format!("smile must be between 1 and {} for a R card", caps.1)), "{}", smile_err);
        let hp_err = run(&mutated("hp", "3")).unwrap_err();
        assert!(hp_err.contains("hp must be between 1 and 2 for a R card"), "{}", hp_err);
        assert!(run(&mutated("hp", "0")).unwrap_err().contains("hp must be between 1 and 2"));
        assert!(run(&mutated("hp", "-1")).unwrap_err().contains("hp must be between 1 and 2"));

        // Fail fast: a field error rejects BEFORE any art is decoded - the
        // garbage art bytes never get the chance to produce their own error
        let mut fields = base_fields();
        field(&mut fields, "rarity", "9");
        fields.insert(String::from("art_00"), b"garbage that would fail decoding".to_vec());
        assert!(run(&fields).unwrap_err().contains("rarity must be 1-3"));

        // Art: both source artworks are required on create; garbage and
        // absurdly small inputs are refused with clear errors
        let mut fields = base_fields();
        fields.remove("art_01");
        assert!(run(&fields).unwrap_err().contains("'art_01' (evolved card artwork) is required"));
        let mut fields = base_fields();
        fields.insert(String::from("art_00"), b"not an image at all".to_vec());
        assert!(run(&fields).unwrap_err().contains("not a decodable image"));
        let mut fields = base_fields();
        fields.insert(String::from("art_00"), seeded_png(32, 32, 7));
        assert!(run(&fields).unwrap_err().contains("at least"));
        // The per-file cap rejects before the decoder ever sees the bytes
        let mut fields = base_fields();
        fields.insert(String::from("art_00"), vec![0u8; MAX_FILE_BYTES + 1]);
        assert!(run(&fields).unwrap_err().contains("per-file limit"));

        // A wrong-sized per-kind override is cropped to target, never rejected
        let mut fields = base_fields();
        fields.insert(String::from("sc_00"), seeded_png(640, 640, 8));
        let override_id = run(&fields).unwrap();
        let sc = image::load_from_memory(&fs::read(format!("{}/sc_00.png", card_dir(override_id))).unwrap()).unwrap();
        assert_eq!((sc.width(), sc.height()), (1024, 512));

        // Character validation via the character route
        let runc = |fields: &Fields| with_permissions(4004, &[permissions::CARD_UPLOAD], || create_character(4004, fields));
        let mut fields = character_fields();
        fields.remove("sign");
        assert!(runc(&fields).unwrap_err().contains("'sign' art is required"));
        let mut fields = character_fields();
        fields.remove("character");
        assert!(runc(&fields).unwrap_err().contains("'character' art is required"));
        let mut fields = character_fields();
        field(&mut fields, "character_name_ruby_en", "");
        assert!(runc(&fields).unwrap_err().contains("English name reading is required"));
        let mut fields = character_fields();
        field(&mut fields, "character_image_color", "red");
        assert!(runc(&fields).unwrap_err().contains("hex color"));
        let mut fields = character_fields();
        fields.insert(String::from("pr"), seeded_png(32, 32, 9));
        assert!(runc(&fields).unwrap_err().contains("at least"));

        // Only the two deliberate successes above wrote rows
        assert_eq!(database::card_count_for_owner(4004), 2);
        wipe(4004);
    }

    #[test]
    fn permission_gates() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(4005);
        wipe(4006);

        let fields = base_fields();
        // No scopes at all
        assert!(create_card(4005, &fields).unwrap_err().contains("permission to upload"));
        assert!(create_character(4005, &character_fields()).unwrap_err().contains("permission to upload"));

        let id = with_permissions(4005, &[permissions::CARD_UPLOAD], || create_card(4005, &fields).unwrap());

        // card.upload edits/deletes its OWN cards but cannot publish
        assert!(with_permissions(4005, &[permissions::CARD_UPLOAD], || set_card_flags(4005, id, Some(true), None)).unwrap_err().contains("permission to publish"));
        let mut published_fields = base_fields();
        field(&mut published_fields, "published", "1");
        assert!(with_permissions(4005, &[permissions::CARD_UPLOAD], || create_card(4005, &published_fields)).unwrap_err().contains("permission to publish"));
        let published_id = with_permissions(4005, &[permissions::CARD_UPLOAD, permissions::CARD_PUBLISH], || create_card(4005, &published_fields).unwrap());
        assert!(database::is_published(published_id));

        // A stranger with card.upload/card.publish can't touch someone else's
        let mut edit = Fields::new();
        field(&mut edit, "name", "Hijacked");
        assert!(with_permissions(4006, &[permissions::CARD_UPLOAD], || update_card(4006, id, &edit)).unwrap_err().contains("permission to edit"));
        assert!(with_permissions(4006, &[permissions::CARD_UPLOAD], || delete_card(4006, id)).unwrap_err().contains("permission to delete"));
        assert!(with_permissions(4006, &[permissions::CARD_PUBLISH], || set_card_flags(4006, id, Some(true), None)).unwrap_err().contains("permission to publish"));

        // card.edit is moderation: manage ANY card
        with_permissions(4006, &[permissions::CARD_EDIT], || {
            update_card(4006, id, &edit).unwrap();
            set_card_flags(4006, id, Some(true), Some(true)).unwrap();
            set_card_flags(4006, id, Some(false), Some(false)).unwrap();
        });
        assert_eq!(database::get_card(id).unwrap()["name"].to_string(), "Hijacked");
        with_permissions(4006, &[permissions::CARD_EDIT], || delete_card(4006, id).unwrap());
        assert!(database::get_card(id).is_none());

        wipe(4005);
        wipe(4006);
    }

    // Present fields replace, absent fields keep, the id never moves, and a
    // replaced art file self-heals its md5
    #[test]
    fn update_edits_in_place() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(4007);

        let fields = base_fields();
        let id = with_permissions(4007, &[permissions::CARD_UPLOAD], || create_card(4007, &fields).unwrap());
        let before = database::get_card(id).unwrap();

        // The sc_00 override is deliberately the wrong size: an update crops
        // it to target just like create does
        let mut edit = Fields::new();
        field(&mut edit, "name", "Renamed");
        field(&mut edit, "skill_effect_type", "7");
        edit.insert(String::from("sc_00"), seeded_png(700, 900, 99));
        with_permissions(4007, &[permissions::CARD_UPLOAD], || update_card(4007, id, &edit).unwrap());

        let after = database::get_card(id).unwrap();
        assert_eq!(after["master_card_id"].as_i64(), Some(id));
        assert_eq!(after["name"].to_string(), String::from("Renamed"));
        assert_eq!(after["skill"]["effect_type"].as_i64(), Some(7));
        assert_eq!(after["skill"]["name"], before["skill"]["name"]);
        assert_eq!(after["smile"], before["smile"]);
        assert_eq!(after["illust_id"], before["illust_id"]);
        assert_eq!(after["art"].len(), 14);

        let old = before["art"].members().find(|art| art["kind"] == "sc" && art["variant"] == "00").unwrap()["md5"].to_string();
        let new = after["art"].members().find(|art| art["kind"] == "sc" && art["variant"] == "00").unwrap()["md5"].to_string();
        assert_ne!(old, new);
        assert_eq!(database::find_asset_by_md5(&old), None);
        assert_eq!(database::find_asset_by_md5(&new), Some(format!("{}/sc_00.png", id)));
        let img = image::load_from_memory(&fs::read(format!("{}/sc_00.png", card_dir(id))).unwrap()).unwrap();
        assert_eq!((img.width(), img.height()), (1024, 512));

        // Re-supplying a source artwork re-derives its whole variant
        let old_c01 = after["art"].members().find(|art| art["kind"] == "c" && art["variant"] == "01").unwrap()["md5"].to_string();
        let mut edit = Fields::new();
        edit.insert(String::from("art_01"), cutout_png(600, 900, 123));
        with_permissions(4007, &[permissions::CARD_UPLOAD], || update_card(4007, id, &edit).unwrap());
        let rederived = database::get_card(id).unwrap();
        assert_eq!(rederived["art"].len(), 14);
        let new_c01 = rederived["art"].members().find(|art| art["kind"] == "c" && art["variant"] == "01").unwrap()["md5"].to_string();
        assert_ne!(new_c01, old_c01);

        // An edit that would break a range is rejected and nothing is written
        let mut bad = Fields::new();
        field(&mut bad, "skill_trigger", "9");
        assert!(with_permissions(4007, &[permissions::CARD_UPLOAD], || update_card(4007, id, &bad)).is_err());
        assert_eq!(database::get_card(id).unwrap()["skill"]["trigger"], after["skill"]["trigger"]);

        wipe(4007);
    }

    // The runtime band must never reach a client that can't fetch the catalog
    #[test]
    fn strip_removes_unsupported_cards_and_their_references() {
        let _lock = crate::runtime::lock_test_data_path();
        let custom = database::FIRST_CARD_ID;
        let mut user = object!{
            "user": {
                "favorite_master_card_id": custom,
                "guest_smile_master_card_id": 10010001
            },
            "card_list": [
                { "master_card_id": 10010001 },
                { "master_card_id": 100010001 },
                { "master_card_id": custom }
            ],
            "deck_list": [
                { "main_card_ids": [custom, 10010001, 0] }
            ]
        };
        strip_unsupported(&mut user);
        assert_eq!(user["card_list"].len(), 2);
        // The imported band stays: those rows are baked into masterdata
        assert!(user["card_list"].members().any(|card| card["master_card_id"] == 100010001));
        assert!(!user["card_list"].members().any(|card| card["master_card_id"] == custom));
        assert_eq!(user["deck_list"][0]["main_card_ids"][0].as_i64(), Some(0));
        assert_eq!(user["deck_list"][0]["main_card_ids"][1].as_i64(), Some(10010001));
        assert_eq!(user["user"]["favorite_master_card_id"].as_i64(), Some(0));
        assert_eq!(user["user"]["guest_smile_master_card_id"].as_i64(), Some(10010001));
    }

    #[test]
    fn bands_do_not_overlap() {
        assert!(is_custom_runtime(database::FIRST_CARD_ID));
        assert!(!is_custom_runtime(141_720_001));
        assert!(crate::router::card::is_custom(141_720_001));
        assert!(!crate::router::card::is_custom(10_010_001));
        assert!(viewer_can_resolve(10_010_001, 0));
        assert!(!viewer_can_resolve(100_010_001, 1));
        assert!(viewer_can_resolve(100_010_001, 2));
    }
}

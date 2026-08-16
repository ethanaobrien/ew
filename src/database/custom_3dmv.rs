use lazy_static::lazy_static;
use rusqlite::params;
use jzon::{array, JsonValue};

use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("custom_3dmv.db", setup_tables);
}

// mv_id is its own namespace: it never appears where a music_id or a card id
// could, so the band only has to avoid colliding with itself. Ids are never
// reused after a delete (high-water mark below), so a client's cached copy of
// a dead id can't get confused with a later upload
pub const FIRST_MV_ID: i64 = 20001;
pub const LAST_MV_ID: i64 = 99_999;

// One JSON blob per MV, in the exact shape /api/custom_3dmv/list serves -
// except `published`, which lives in its own column (the catalog filter
// queries it) and is only injected for the webui manage view
fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS mvs (
    mv_id      BIGINT NOT NULL PRIMARY KEY,
    music_id   BIGINT NOT NULL,
    owner_id   BIGINT NOT NULL,
    mv         TEXT NOT NULL,
    published  INT NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS revision (
    id          INT NOT NULL PRIMARY KEY,
    revision    BIGINT NOT NULL,
    last_mv_id  BIGINT NOT NULL
);
    ").unwrap();
}

pub fn get_revision() -> i64 {
    DATABASE.lock_and_select("SELECT revision FROM revision WHERE id=1", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0)
}

// Bumped on every upload/update/delete/publish change so the client can tell
// its cached catalog is stale
pub fn bump_revision() {
    DATABASE.lock_and_exec("INSERT INTO revision (id, revision, last_mv_id) VALUES (1, 1, 0) ON CONFLICT(id) DO UPDATE SET revision=revision+1", params!());
}

// last_mv_id is the high-water mark and only ever rises, so MAX() over the
// live rows is a floor, not the answer
pub fn next_mv_id() -> i64 {
    let issued = DATABASE.lock_and_select("SELECT last_mv_id FROM revision WHERE id=1", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    let max = DATABASE.lock_and_select("SELECT MAX(mv_id) FROM mvs", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    std::cmp::max(std::cmp::max(issued, max), FIRST_MV_ID - 1) + 1
}

pub fn insert_mv(mv_id: i64, music_id: i64, owner_id: i64, mv: &JsonValue, published: bool) {
    DATABASE.lock_and_exec(
        "INSERT INTO mvs (mv_id, music_id, owner_id, mv, published) VALUES (?1, ?2, ?3, ?4, ?5)",
        params!(mv_id, music_id, owner_id, jzon::stringify(mv.clone()), published as i64)
    );
    DATABASE.lock_and_exec("INSERT INTO revision (id, revision, last_mv_id) VALUES (1, 0, ?1) ON CONFLICT(id) DO UPDATE SET last_mv_id=?1", params!(mv_id));
}

// The catalog blob only. The owner and the published flag live in their own
// columns and are untouched here; music_id is fixed for the life of the MV
pub fn update_mv(mv_id: i64, mv: &JsonValue) {
    DATABASE.lock_and_exec("UPDATE mvs SET mv=?1 WHERE mv_id=?2", params!(jzon::stringify(mv.clone()), mv_id));
}

pub fn delete_mv(mv_id: i64) {
    DATABASE.lock_and_exec("DELETE FROM mvs WHERE mv_id=?1", params!(mv_id));
}

pub fn get_mv(mv_id: i64) -> Option<JsonValue> {
    let mv = DATABASE.lock_and_select("SELECT mv FROM mvs WHERE mv_id=?1", params!(mv_id)).ok()?;
    jzon::parse(&mv).ok()
}

pub fn get_mv_owner(mv_id: i64) -> Option<i64> {
    DATABASE.lock_and_select("SELECT owner_id FROM mvs WHERE mv_id=?1", params!(mv_id)).ok()?.parse::<i64>().ok()
}

pub fn get_mv_music_id(mv_id: i64) -> Option<i64> {
    DATABASE.lock_and_select("SELECT music_id FROM mvs WHERE mv_id=?1", params!(mv_id)).ok()?.parse::<i64>().ok()
}

pub fn is_published(mv_id: i64) -> bool {
    DATABASE.lock_and_select("SELECT published FROM mvs WHERE mv_id=?1", params!(mv_id)).unwrap_or_default() == "1"
}

pub fn set_published(mv_id: i64, published: bool) {
    DATABASE.lock_and_exec("UPDATE mvs SET published=?1 WHERE mv_id=?2", params!(published as i64, mv_id));
}

pub fn mv_count_for_owner(owner_id: i64) -> i64 {
    DATABASE.lock_and_select_type::<i64>("SELECT COUNT(*) FROM mvs WHERE owner_id=?1", params!(owner_id)).unwrap_or(0)
}

fn parse_blobs(rows: JsonValue) -> JsonValue {
    let mut rv = array![];
    for data in rows.members() {
        if let Ok(parsed) = jzon::parse(&data.to_string()) {
            rv.push(parsed).unwrap();
        }
    }
    rv
}

// The MV catalog this user is served: every published MV plus their own
// drafts, filtered against `music_ids` - the music ids the SAME user's
// custom-song catalog delivers. The closure is what keeps the response
// referentially sound: a served MV must never name a music_id the song
// catalog failed to deliver (a published MV for someone else's private song
// stays invisible)
pub fn get_mvs_for_user(user_id: i64, music_ids: &[i64]) -> JsonValue {
    let rows = parse_blobs(DATABASE.lock_and_select_all(
        "SELECT mv FROM mvs WHERE published=1 OR owner_id=?1 ORDER BY mv_id",
        params!(user_id)
    ).unwrap_or(array![]));
    let mut rv = array![];
    for mv in rows.members() {
        if music_ids.contains(&mv["music_id"].as_i64().unwrap_or(0)) {
            rv.push(mv.clone()).unwrap();
        }
    }
    rv
}

// MV blobs plus the flag column, for the webui manage view
pub fn get_mvs_by_owner(owner_id: i64) -> JsonValue {
    let rows = parse_blobs(DATABASE.lock_and_select_all("SELECT mv FROM mvs WHERE owner_id=?1 ORDER BY mv_id", params!(owner_id)).unwrap_or(array![]));
    let mut rv = array![];
    for mv in rows.members() {
        let mut mv = mv.clone();
        mv["published"] = is_published(mv["mv_id"].as_i64().unwrap_or(0)).into();
        rv.push(mv).unwrap();
    }
    rv
}

// The webui MV browser: published MVs whose song the viewer can see, plus the
// owner id so the page can label the uploader
pub fn get_browse_mvs(viewer_music_ids: &[i64]) -> JsonValue {
    let rows = parse_blobs(DATABASE.lock_and_select_all("SELECT mv FROM mvs WHERE published=1 ORDER BY mv_id", params!()).unwrap_or(array![]));
    let mut rv = array![];
    for mv in rows.members() {
        if !viewer_music_ids.contains(&mv["music_id"].as_i64().unwrap_or(0)) {
            continue;
        }
        let mut mv = mv.clone();
        mv["owner_id"] = get_mv_owner(mv["mv_id"].as_i64().unwrap_or(0)).unwrap_or(0).into();
        rv.push(mv).unwrap();
    }
    rv
}

// Every MV attached to a song, for the delete cascade
pub fn mv_ids_for_music(music_id: i64) -> Vec<i64> {
    let rows = DATABASE.lock_and_select_all("SELECT mv_id FROM mvs WHERE music_id=?1 ORDER BY mv_id", params!(music_id)).unwrap_or(array![]);
    rows.members().filter_map(|id| id.as_i64()).collect()
}

// Which of these candidate ids no longer exist. Only the MV band is ever
// considered and ids are never reused, so a wipe is final. An MV that's
// merely unpublished still has its row - only genuinely deleted ids return
pub fn dead_mv_ids(candidates: &JsonValue) -> JsonValue {
    let mut ids: Vec<i64> = Vec::new();
    for id in candidates.members() {
        let Some(id) = id.as_i64() else { continue; };
        if (FIRST_MV_ID..=LAST_MV_ID).contains(&id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return array![];
    }
    let list = ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let alive = DATABASE.lock_and_select_all(&format!("SELECT mv_id FROM mvs WHERE mv_id IN ({})", list), params!()).unwrap_or(array![]);
    let mut rv = array![];
    for id in ids {
        if !alive.contains(id) {
            rv.push(id).unwrap();
        }
    }
    rv
}

// Every stored catalog blob, unparsed and unfiltered. Only the startup blob
// sweep needs the whole table, and a read failure must never look like an
// empty catalog (the sweep would then delete every blob), so this returns
// None rather than an empty array on error
pub fn all_mv_blobs() -> Option<JsonValue> {
    DATABASE.lock_and_select_all("SELECT mv FROM mvs ORDER BY mv_id", params!()).ok()
}

// Blobs are content-addressed and may be shared between MVs (or roles), so
// every candidate row is checked - a single-row scan could land on a
// coincidental substring match and miss the real reference
pub fn blob_in_use(md5: &str) -> bool {
    let rows = DATABASE.lock_and_select_all("SELECT mv FROM mvs WHERE mv LIKE ?1", params!(format!("%{}%", md5))).unwrap_or(array![]);
    for blob in rows.members() {
        if let Ok(mv) = jzon::parse(&blob.to_string()) {
            if mv["files"].members().any(|file| file["md5"].as_str() == Some(md5)) {
                return true;
            }
        }
    }
    false
}

// Two-step content-addressed lookup for the data route: the LIKE scan finds a
// candidate row cheaply, then the files array confirms the md5 is really a
// stored file of a live MV (and not a substring coincidence elsewhere in the
// blob). The blob path itself derives from the md5
pub fn find_blob_by_md5(md5: &str) -> bool {
    blob_in_use(md5)
}

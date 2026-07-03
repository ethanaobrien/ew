use lazy_static::lazy_static;
use rusqlite::params;
use jzon::{array, JsonValue};

use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("custom_songs.db", setup_tables);
}

// Song visibility: "public" (everyone), "private" (owner only) or "shared"
// (owner plus the user ids in shared_users)
pub const VISIBILITIES: &[&str] = &["public", "private", "shared"];

// master_group_id must be a real GroupMst id whose category matches the song's
// band_category, or the client's music-library group filter throws
// KeyNotFoundException (0 is never a valid GroupMst id). Custom songs go into
// each band's misc / "その他" catch-all group so they don't masquerade as an
// official sub-unit. Bands with no misc group (and OTHER/unknown) fall back to
// 9999, the OTHER catch-all
pub fn band_group_id(band_category: &str) -> i64 {
    match band_category {
        "MUSE" => 199,
        "AQOURS" => 299,
        "NIJIGAKU" => 399,
        "LIELLA" => 499,
        "HASUNOSORA" => 599,
        _ => 9999
    }
}

fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS songs (
    music_id            BIGINT NOT NULL PRIMARY KEY,
    owner_id            BIGINT NOT NULL,
    song                TEXT NOT NULL,
    visibility          TEXT NOT NULL DEFAULT 'public',
    downloads_disabled  INT NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS shared_users (
    music_id  BIGINT NOT NULL,
    user_id   BIGINT NOT NULL,
    PRIMARY KEY (music_id, user_id)
);
CREATE TABLE IF NOT EXISTS revision (
    id             INT NOT NULL PRIMARY KEY,
    revision       BIGINT NOT NULL,
    last_music_id  BIGINT NOT NULL
);
    ").unwrap();
    // Upgrade pre-visibility databases. Existing songs stay public
    if conn.prepare("SELECT visibility FROM songs LIMIT 1;").is_err() {
        println!("Upgrading custom song table");
        conn.execute("ALTER TABLE songs ADD COLUMN visibility TEXT NOT NULL DEFAULT 'public';", []).unwrap();
    }
    // Upgrade pre-download-toggle databases. Existing songs stay downloadable
    if conn.prepare("SELECT downloads_disabled FROM songs LIMIT 1;").is_err() {
        println!("Upgrading custom song table (downloads)");
        conn.execute("ALTER TABLE songs ADD COLUMN downloads_disabled INT NOT NULL DEFAULT 0;", []).unwrap();
    }
    // Rewrite blobs that stored the old invalid master_group_id 0 to the band's
    // real GroupMst id (the client's group filter crashes on 0). The blob is
    // served verbatim, so this must be persisted, not just applied at read time
    let rows: Vec<(i64, String)> = {
        let mut stmt = conn.prepare("SELECT music_id, song FROM songs").unwrap();
        let mapped = stmt.query_map([], |row| Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))).unwrap();
        mapped.filter_map(|r| r.ok()).collect()
    };
    let mut fixed = 0;
    for (music_id, blob) in rows {
        let mut song = jzon::parse(&blob).unwrap();
        if song["master_group_id"].as_i64() == Some(0) {
            song["master_group_id"] = band_group_id(&song["band_category"].to_string()).into();
            conn.execute("UPDATE songs SET song=?1 WHERE music_id=?2", params!(jzon::stringify(song), music_id)).unwrap();
            fixed += 1;
        }
    }
    if fixed > 0 {
        println!("Upgrading custom song table (master_group_id): {} row(s)", fixed);
    }
}

pub fn get_revision() -> i64 {
    DATABASE.lock_and_select("SELECT revision FROM revision WHERE id=1", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0)
}

// Bumped on every upload/delete/visibility change so the client can invalidate its cache
pub fn bump_revision() {
    DATABASE.lock_and_exec("INSERT INTO revision (id, revision, last_music_id) VALUES (1, 1, 0) ON CONFLICT(id) DO UPDATE SET revision=revision+1", params!());
}

// music_ids are assigned sequentially starting at 10000. live_id == music_id.
// The official music mst occupies 4-digit ids up to 9032 (School Idol Musical
// + 異次元フェス) along with bgm 310090xx/320090xx, so the custom range gets
// the 5-digit space to itself (10000 -> bgm 31010000/32010000). Ids are never
// reused after a delete, so a client's cached copy of a dead id can't get
// confused with a new upload
pub const FIRST_MUSIC_ID: i64 = 10000;

pub fn next_music_id() -> i64 {
    let issued = DATABASE.lock_and_select("SELECT last_music_id FROM revision WHERE id=1", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    let max = DATABASE.lock_and_select("SELECT MAX(music_id) FROM songs", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    std::cmp::max(std::cmp::max(issued, max), FIRST_MUSIC_ID - 1) + 1
}

pub fn insert_song(music_id: i64, owner_id: i64, song: &JsonValue, visibility: &str, shared_with: &JsonValue, downloads_disabled: bool) {
    DATABASE.lock_and_exec("INSERT INTO songs (music_id, owner_id, song, visibility, downloads_disabled) VALUES (?1, ?2, ?3, ?4, ?5)", params!(music_id, owner_id, jzon::stringify(song.clone()), visibility, downloads_disabled as i64));
    DATABASE.lock_and_exec("INSERT INTO revision (id, revision, last_music_id) VALUES (1, 0, ?1) ON CONFLICT(id) DO UPDATE SET last_music_id=?1", params!(music_id));
    set_shared_users(music_id, shared_with);
}

pub fn delete_song(music_id: i64) {
    DATABASE.lock_and_exec("DELETE FROM songs WHERE music_id=?1", params!(music_id));
    DATABASE.lock_and_exec("DELETE FROM shared_users WHERE music_id=?1", params!(music_id));
}

pub fn get_song(music_id: i64) -> Option<JsonValue> {
    let song = DATABASE.lock_and_select("SELECT song FROM songs WHERE music_id=?1", params!(music_id)).ok()?;
    jzon::parse(&song).ok()
}

pub fn get_song_owner(music_id: i64) -> Option<i64> {
    DATABASE.lock_and_select("SELECT owner_id FROM songs WHERE music_id=?1", params!(music_id)).ok()?.parse::<i64>().ok()
}

pub fn set_visibility(music_id: i64, visibility: &str, shared_with: &JsonValue) {
    DATABASE.lock_and_exec("UPDATE songs SET visibility=?1 WHERE music_id=?2", params!(visibility, music_id));
    set_shared_users(music_id, shared_with);
}

pub fn set_downloads_disabled(music_id: i64, downloads_disabled: bool) {
    DATABASE.lock_and_exec("UPDATE songs SET downloads_disabled=?1 WHERE music_id=?2", params!(downloads_disabled as i64, music_id));
}

fn get_downloads_disabled(music_id: i64) -> bool {
    DATABASE.lock_and_select("SELECT downloads_disabled FROM songs WHERE music_id=?1", params!(music_id)).unwrap_or_default() == "1"
}

fn set_shared_users(music_id: i64, shared_with: &JsonValue) {
    DATABASE.lock_and_exec("DELETE FROM shared_users WHERE music_id=?1", params!(music_id));
    for id in shared_with.members() {
        DATABASE.lock_and_exec("INSERT OR IGNORE INTO shared_users (music_id, user_id) VALUES (?1, ?2)", params!(music_id, id.as_i64().unwrap()));
    }
}

fn get_visibility(music_id: i64) -> String {
    DATABASE.lock_and_select("SELECT visibility FROM songs WHERE music_id=?1", params!(music_id)).unwrap_or(String::from("public"))
}

fn get_shared_users(music_id: i64) -> JsonValue {
    DATABASE.lock_and_select_all("SELECT user_id FROM shared_users WHERE music_id=?1 ORDER BY user_id", params!(music_id)).unwrap_or(array![])
}

// The catalog a given user is allowed to see: public songs, their own songs,
// and songs shared with them
pub fn get_songs_for_user(user_id: i64) -> JsonValue {
    let songs = DATABASE.lock_and_select_all("
    SELECT song FROM songs
    WHERE visibility='public' OR owner_id=?1
    OR (visibility='shared' AND music_id IN (SELECT music_id FROM shared_users WHERE user_id=?1))
    ORDER BY music_id", params!(user_id)).unwrap_or(array![]);
    let mut rv = array![];
    for data in songs.members() {
        rv.push(jzon::parse(&data.to_string()).unwrap()).unwrap();
    }
    rv
}

// Song blobs plus the visibility fields, for the webui manage view
pub fn get_songs_by_owner(owner_id: i64) -> JsonValue {
    let songs = DATABASE.lock_and_select_all("SELECT song FROM songs WHERE owner_id=?1 ORDER BY music_id", params!(owner_id)).unwrap_or(array![]);
    let mut rv = array![];
    for data in songs.members() {
        let mut song = jzon::parse(&data.to_string()).unwrap();
        let music_id = song["music_id"].as_i64().unwrap();
        song["visibility"] = get_visibility(music_id).into();
        song["shared_with"] = get_shared_users(music_id);
        song["downloads_disabled"] = get_downloads_disabled(music_id).into();
        rv.push(song).unwrap();
    }
    rv
}

// The webui song browser: everything the viewer is allowed to see under the
// visibility rules, plus the fields the browser page needs. Anonymous viewers
// (no webui session) see the public catalog only
pub fn get_browse_songs(viewer: Option<i64>) -> JsonValue {
    let viewer = viewer.unwrap_or(0);
    let songs = DATABASE.lock_and_select_all("
    SELECT song FROM songs
    WHERE visibility='public' OR owner_id=?1
    OR (visibility='shared' AND music_id IN (SELECT music_id FROM shared_users WHERE user_id=?1))
    ORDER BY music_id", params!(viewer)).unwrap_or(array![]);
    let mut rv = array![];
    for data in songs.members() {
        let mut song = jzon::parse(&data.to_string()).unwrap();
        let music_id = song["music_id"].as_i64().unwrap();
        let owner = get_song_owner(music_id).unwrap_or(0);
        song["owner_id"] = owner.into();
        song["mine"] = (owner == viewer).into();
        song["downloads_disabled"] = get_downloads_disabled(music_id).into();
        rv.push(song).unwrap();
    }
    rv
}

// Whether this viewer may download the song's export package: they must be
// able to SEE it (same rules as the catalog - a private song 404s rather than
// admitting it exists), and downloads must be enabled unless they own it
pub fn export_allowed(music_id: i64, viewer: Option<i64>) -> Result<(), &'static str> {
    let Some(owner) = get_song_owner(music_id) else {
        return Err("Song not found");
    };
    let viewer = viewer.unwrap_or(0);
    if owner == viewer {
        return Ok(());
    }
    let visible = match get_visibility(music_id).as_str() {
        "public" => true,
        "shared" => get_shared_users(music_id).contains(viewer),
        _ => false
    };
    if !visible {
        return Err("Song not found");
    }
    if get_downloads_disabled(music_id) {
        return Err("The uploader has disabled downloads for this song");
    }
    Ok(())
}

pub fn get_music_ids_for_user(user_id: i64) -> JsonValue {
    DATABASE.lock_and_select_all("
    SELECT music_id FROM songs
    WHERE visibility='public' OR owner_id=?1
    OR (visibility='shared' AND music_id IN (SELECT music_id FROM shared_users WHERE user_id=?1))
    ORDER BY music_id", params!(user_id)).unwrap_or(array![])
}

// Which of these candidate ids no longer exist in the catalog. Only the custom
// range is ever considered, so official songs can't come back from this. A song
// that's merely private/shared still has its row - only genuinely deleted ids
// (which are never reused) are returned
pub fn dead_music_ids(candidates: &JsonValue) -> JsonValue {
    let mut ids: Vec<i64> = Vec::new();
    for id in candidates.members() {
        let Some(id) = id.as_i64() else { continue; };
        if id >= FIRST_MUSIC_ID && !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return array![];
    }
    let list = ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let alive = DATABASE.lock_and_select_all(&format!("SELECT music_id FROM songs WHERE music_id IN ({})", list), params!()).unwrap_or(array![]);
    let mut rv = array![];
    for id in ids {
        if !alive.contains(id) {
            rv.push(id).unwrap();
        }
    }
    rv
}

// Audio files are content-addressed and may be shared between songs
pub fn audio_in_use(md5: &str, ignored_music_id: i64) -> bool {
    DATABASE.lock_and_select("SELECT music_id FROM songs WHERE music_id!=?1 AND song LIKE ?2", params!(ignored_music_id, format!("%{}%", md5))).is_ok()
}

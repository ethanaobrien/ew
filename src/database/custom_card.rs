use lazy_static::lazy_static;
use rusqlite::params;
use jzon::{array, JsonValue};

use crate::sql::SQLite;

lazy_static! {
    static ref DATABASE: SQLite = SQLite::new("custom_cards.db", setup_tables);
}

// master_card_id == illust_prefix * 10000 + seq, and illust_id is derived from
// the same pair as {prefix:05}_{seq:04}_{00|01}. Official cards are 8-digit
// (prefix 1001-4014), the baked SIF1 import owns prefixes 10000-14999 (rows in
// client masterdata), so runtime uploads start at prefix 15000. seq 0 never
// exists - official illust ids start at 0001 - so an id landing on a prefix
// boundary is skipped, not issued
pub const FIRST_ILLUST_PREFIX: i64 = 15000;
pub const FIRST_CARD_ID: i64 = FIRST_ILLUST_PREFIX * 10000 + 1;
pub const LAST_CARD_ID: i64 = 999_999_999;

// Official characters are 1001-4014, the SIF1 import took 5001-5172 and
// 6001-6009 - client masterdata tops out at 6009, so runtime characters start
// at 7001. The ceiling keeps the id 5-digit (sign_{0:D5} asset naming) and
// below the 99999 sentinel SDCharacter substitutes for OTHER-category
// characters
pub const FIRST_CHARACTER_ID: i64 = 7001;
pub const LAST_CHARACTER_ID: i64 = 99_998;

// Cards and characters are one JSON blob each, in the exact shape
// /api/custom_card/list serves - except `published`/`obtainable`/`rarity`,
// which live in their own columns (the draw pool and the catalog filter query
// them) and are injected into the served object where the wire wants them
fn setup_tables(conn: &rusqlite::Connection) {
    conn.execute_batch("
CREATE TABLE IF NOT EXISTS cards (
    master_card_id       BIGINT NOT NULL PRIMARY KEY,
    master_character_id  BIGINT NOT NULL,
    owner_id             BIGINT NOT NULL,
    card                 TEXT NOT NULL,
    rarity               INT NOT NULL DEFAULT 1,
    published            INT NOT NULL DEFAULT 0,
    obtainable           INT NOT NULL DEFAULT 0
);
CREATE TABLE IF NOT EXISTS characters (
    master_character_id  BIGINT NOT NULL PRIMARY KEY,
    owner_id             BIGINT NOT NULL,
    character            TEXT NOT NULL
);
CREATE TABLE IF NOT EXISTS revision (
    id                 INT NOT NULL PRIMARY KEY,
    revision           BIGINT NOT NULL,
    last_card_id       BIGINT NOT NULL,
    last_character_id  BIGINT NOT NULL
);
    ").unwrap();
}

pub fn get_revision() -> i64 {
    DATABASE.lock_and_select("SELECT revision FROM revision WHERE id=1", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0)
}

// Bumped on every create/update/delete/publish/obtainable change so the client
// can tell its cached catalog is stale
pub fn bump_revision() {
    DATABASE.lock_and_exec("INSERT INTO revision (id, revision, last_card_id, last_character_id) VALUES (1, 1, 0, 0) ON CONFLICT(id) DO UPDATE SET revision=revision+1", params!());
}

// Ids are never reused after a delete: a client that cached a dead id can't
// confuse it with a later upload, and a player's stored card row for a deleted
// card can't silently become a different card. last_card_id is the high-water
// mark and only ever rises, so MAX() over the live rows is a floor, not the
// answer
pub fn next_card_id() -> i64 {
    let issued = DATABASE.lock_and_select("SELECT last_card_id FROM revision WHERE id=1", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    let max = DATABASE.lock_and_select("SELECT MAX(master_card_id) FROM cards", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    let mut rv = std::cmp::max(std::cmp::max(issued, max), FIRST_CARD_ID - 1) + 1;
    // seq 0 doesn't exist in the illust naming scheme
    if rv % 10000 == 0 {
        rv += 1;
    }
    rv
}

pub fn next_character_id() -> i64 {
    let issued = DATABASE.lock_and_select("SELECT last_character_id FROM revision WHERE id=1", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    let max = DATABASE.lock_and_select("SELECT MAX(master_character_id) FROM characters", params!()).unwrap_or_default().parse::<i64>().unwrap_or(0);
    std::cmp::max(std::cmp::max(issued, max), FIRST_CHARACTER_ID - 1) + 1
}

pub fn insert_card(master_card_id: i64, master_character_id: i64, owner_id: i64, card: &JsonValue, published: bool, obtainable: bool) {
    DATABASE.lock_and_exec(
        "INSERT INTO cards (master_card_id, master_character_id, owner_id, card, rarity, published, obtainable) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        params!(master_card_id, master_character_id, owner_id, jzon::stringify(card.clone()), card["rarity"].as_i64().unwrap_or(1), published as i64, obtainable as i64)
    );
    DATABASE.lock_and_exec("INSERT INTO revision (id, revision, last_card_id, last_character_id) VALUES (1, 0, ?1, 0) ON CONFLICT(id) DO UPDATE SET last_card_id=?1", params!(master_card_id));
}

pub fn insert_character(master_character_id: i64, owner_id: i64, character: &JsonValue) {
    DATABASE.lock_and_exec(
        "INSERT INTO characters (master_character_id, owner_id, character) VALUES (?1, ?2, ?3)",
        params!(master_character_id, owner_id, jzon::stringify(character.clone()))
    );
    DATABASE.lock_and_exec("INSERT INTO revision (id, revision, last_card_id, last_character_id) VALUES (1, 0, 0, ?1) ON CONFLICT(id) DO UPDATE SET last_character_id=?1", params!(master_character_id));
}

// The catalog blob only. The owner and the published/obtainable flags live in
// their own columns and are untouched here; rarity tracks the blob
pub fn update_card(master_card_id: i64, card: &JsonValue) {
    DATABASE.lock_and_exec("UPDATE cards SET card=?1, rarity=?2 WHERE master_card_id=?3", params!(jzon::stringify(card.clone()), card["rarity"].as_i64().unwrap_or(1), master_card_id));
}

pub fn update_character(master_character_id: i64, character: &JsonValue) {
    DATABASE.lock_and_exec("UPDATE characters SET character=?1 WHERE master_character_id=?2", params!(jzon::stringify(character.clone()), master_character_id));
}

pub fn delete_card(master_card_id: i64) {
    DATABASE.lock_and_exec("DELETE FROM cards WHERE master_card_id=?1", params!(master_card_id));
}

pub fn delete_character(master_character_id: i64) {
    DATABASE.lock_and_exec("DELETE FROM characters WHERE master_character_id=?1", params!(master_character_id));
}

// The stored blob with the column-backed wire field injected
fn card_with_flags(mut card: JsonValue) -> JsonValue {
    let id = card["master_card_id"].as_i64().unwrap_or(0);
    card["obtainable"] = is_obtainable(id).into();
    card
}

pub fn get_card(master_card_id: i64) -> Option<JsonValue> {
    let card = DATABASE.lock_and_select("SELECT card FROM cards WHERE master_card_id=?1", params!(master_card_id)).ok()?;
    Some(card_with_flags(jzon::parse(&card).ok()?))
}

pub fn get_character(master_character_id: i64) -> Option<JsonValue> {
    let character = DATABASE.lock_and_select("SELECT character FROM characters WHERE master_character_id=?1", params!(master_character_id)).ok()?;
    jzon::parse(&character).ok()
}

pub fn get_card_owner(master_card_id: i64) -> Option<i64> {
    DATABASE.lock_and_select("SELECT owner_id FROM cards WHERE master_card_id=?1", params!(master_card_id)).ok()?.parse::<i64>().ok()
}

pub fn get_character_owner(master_character_id: i64) -> Option<i64> {
    DATABASE.lock_and_select("SELECT owner_id FROM characters WHERE master_character_id=?1", params!(master_character_id)).ok()?.parse::<i64>().ok()
}

// The character a runtime card belongs to, straight out of its own column.
// guest::proxy_card_id resolves an unviewable card through this, so it stays a
// plain lookup and never a decode of the blob
pub fn character_of(master_card_id: i64) -> Option<i64> {
    DATABASE.lock_and_select("SELECT master_character_id FROM cards WHERE master_card_id=?1", params!(master_card_id)).ok()?.parse::<i64>().ok()
}

pub fn is_published(master_card_id: i64) -> bool {
    DATABASE.lock_and_select("SELECT published FROM cards WHERE master_card_id=?1", params!(master_card_id)).unwrap_or_default() == "1"
}

pub fn set_published(master_card_id: i64, published: bool) {
    DATABASE.lock_and_exec("UPDATE cards SET published=?1 WHERE master_card_id=?2", params!(published as i64, master_card_id));
}

pub fn is_obtainable(master_card_id: i64) -> bool {
    DATABASE.lock_and_select("SELECT obtainable FROM cards WHERE master_card_id=?1", params!(master_card_id)).unwrap_or_default() == "1"
}

pub fn set_obtainable(master_card_id: i64, obtainable: bool) {
    DATABASE.lock_and_exec("UPDATE cards SET obtainable=?1 WHERE master_card_id=?2", params!(obtainable as i64, master_card_id));
}

pub fn has_character(master_character_id: i64) -> bool {
    DATABASE.lock_and_select("SELECT master_character_id FROM characters WHERE master_character_id=?1", params!(master_character_id)).is_ok()
}

pub fn card_count_for_owner(owner_id: i64) -> i64 {
    DATABASE.lock_and_select_type::<i64>("SELECT COUNT(*) FROM cards WHERE owner_id=?1", params!(owner_id)).unwrap_or(0)
}

// How many cards still point at this character. A referenced character can't
// be deleted
pub fn cards_using_character(master_character_id: i64) -> i64 {
    DATABASE.lock_and_select_type::<i64>("SELECT COUNT(*) FROM cards WHERE master_character_id=?1", params!(master_character_id)).unwrap_or(0)
}

// A custom character is publicly visible when a published card references it -
// that's also what lets another uploader build a card on it
pub fn character_publicly_visible(master_character_id: i64) -> bool {
    DATABASE.lock_and_select_type::<i64>(
        "SELECT COUNT(*) FROM cards WHERE master_character_id=?1 AND published=1",
        params!(master_character_id)
    ).unwrap_or(0) > 0
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

// The card catalog this user is served: every published card, their own
// drafts, and any card their game account already owns (`owned` - so an
// unpublish never leaves a player holding an id their client can't resolve)
pub fn get_cards_for_user(user_id: i64, owned: &[i64]) -> JsonValue {
    let rows = parse_blobs(DATABASE.lock_and_select_all(
        "SELECT card FROM cards WHERE published=1 OR owner_id=?1 ORDER BY master_card_id",
        params!(user_id)
    ).unwrap_or(array![]));
    let mut rv = array![];
    let mut ids: Vec<i64> = Vec::new();
    for card in rows.members() {
        ids.push(card["master_card_id"].as_i64().unwrap_or(0));
        rv.push(card_with_flags(card.clone())).unwrap();
    }
    for id in owned {
        if ids.contains(id) {
            continue;
        }
        if let Some(card) = get_card(*id) {
            rv.push(card).unwrap();
        }
    }
    rv
}

// The character catalog rides along with the cards: a custom character is
// included exactly when a served card references it, or the requester owns it
// (so a draft character never leaks, and the catalog is referentially closed -
// a served card can never name a master_character_id the same response failed
// to deliver)
pub fn get_characters_for_cards(user_id: i64, cards: &JsonValue) -> JsonValue {
    let mut ids: Vec<i64> = Vec::new();
    for card in cards.members() {
        let id = card["master_character_id"].as_i64().unwrap_or(0);
        if (FIRST_CHARACTER_ID..=LAST_CHARACTER_ID).contains(&id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    let own = DATABASE.lock_and_select_all("SELECT master_character_id FROM characters WHERE owner_id=?1 ORDER BY master_character_id", params!(user_id)).unwrap_or(array![]);
    for id in own.members() {
        let id = id.as_i64().unwrap_or(0);
        if !ids.contains(&id) {
            ids.push(id);
        }
    }
    ids.sort();
    let mut rv = array![];
    for id in ids {
        if let Some(character) = get_character(id) {
            rv.push(character).unwrap();
        }
    }
    rv
}

// The custom characters this user may build a card on (mirrors
// validate_character_ref): their own, plus any that is publicly visible
// through a published card. Feeds the webui's character picker
pub fn get_selectable_characters(user_id: i64) -> JsonValue {
    parse_blobs(DATABASE.lock_and_select_all("
    SELECT character FROM characters
    WHERE owner_id=?1 OR master_character_id IN (SELECT master_character_id FROM cards WHERE published=1)
    ORDER BY master_character_id", params!(user_id)).unwrap_or(array![]))
}

// Card blobs plus the flag columns, for the webui manage view
pub fn get_cards_by_owner(owner_id: i64) -> JsonValue {
    let rows = parse_blobs(DATABASE.lock_and_select_all("SELECT card FROM cards WHERE owner_id=?1 ORDER BY master_card_id", params!(owner_id)).unwrap_or(array![]));
    let mut rv = array![];
    for card in rows.members() {
        let mut card = card_with_flags(card.clone());
        card["published"] = is_published(card["master_card_id"].as_i64().unwrap_or(0)).into();
        rv.push(card).unwrap();
    }
    rv
}

pub fn get_characters_by_owner(owner_id: i64) -> JsonValue {
    parse_blobs(DATABASE.lock_and_select_all("SELECT character FROM characters WHERE owner_id=?1 ORDER BY master_character_id", params!(owner_id)).unwrap_or(array![]))
}

// The webui card browser: every published card, plus the owner id so the page
// can label the uploader
pub fn get_browse_cards() -> JsonValue {
    let rows = parse_blobs(DATABASE.lock_and_select_all("SELECT card FROM cards WHERE published=1 ORDER BY master_card_id", params!()).unwrap_or(array![]));
    let mut rv = array![];
    for card in rows.members() {
        let mut card = card_with_flags(card.clone());
        card["owner_id"] = get_card_owner(card["master_card_id"].as_i64().unwrap_or(0)).unwrap_or(0).into();
        rv.push(card).unwrap();
    }
    rv
}

// Which of these candidate ids no longer exist. Only the runtime band is ever
// considered, and ids are never reused, so official (or imported) cards can't
// come back from this and a wipe is final. A card that's merely unpublished
// still has its row - only genuinely deleted ids are returned
pub fn dead_card_ids(candidates: &JsonValue) -> JsonValue {
    let mut ids: Vec<i64> = Vec::new();
    for id in candidates.members() {
        let Some(id) = id.as_i64() else { continue; };
        if (FIRST_CARD_ID..=LAST_CARD_ID).contains(&id) && !ids.contains(&id) {
            ids.push(id);
        }
    }
    if ids.is_empty() {
        return array![];
    }
    let list = ids.iter().map(|id| id.to_string()).collect::<Vec<_>>().join(",");
    let alive = DATABASE.lock_and_select_all(&format!("SELECT master_card_id FROM cards WHERE master_card_id IN ({})", list), params!()).unwrap_or(array![]);
    let mut rv = array![];
    for id in ids {
        if !alive.contains(id) {
            rv.push(id).unwrap();
        }
    }
    rv
}

// The published + obtainable pool the custom gacha banner draws from, per
// rarity
pub fn obtainable_card_ids(rarity: i64) -> Vec<i64> {
    let rows = DATABASE.lock_and_select_all(
        "SELECT master_card_id FROM cards WHERE published=1 AND obtainable=1 AND rarity=?1 ORDER BY master_card_id",
        params!(rarity)
    ).unwrap_or(array![]);
    rows.members().filter_map(|id| id.as_i64()).collect()
}

// Resolve a content-addressed art md5 to the file under custom_cards/ that
// currently holds those bytes: card art lives at {card_id}/{kind}_{variant}.png,
// character art at characters/{character_id}/{kind}.png. Art is stored per
// entity (not a shared md5 store) and the catalog md5 always tracks the
// on-disk bytes - so this is the index the /custom_card/data/{md5} route
// serves from, and it self-heals: a replaced file gets a new md5 and the old
// one simply stops resolving
// Resolve a voiceline md5 to its ogg under custom_cards/. Voicelines live in
// the character blob's `voice` array and on disk under the character's own
// voice/ subdirectory, so they share the art routes' self-healing property
pub fn find_voice_by_md5(md5: &str) -> Option<String> {
    let blob = DATABASE.lock_and_select("SELECT character FROM characters WHERE character LIKE ?1", params!(format!("%{}%", md5))).ok()?;
    let character = jzon::parse(&blob).ok()?;
    let id = character["master_character_id"].as_i64()?;
    for line in character["voice"].members() {
        if line["md5"].as_str() == Some(md5) {
            return Some(format!("characters/{}/voice/{}.ogg", id, md5));
        }
    }
    None
}

pub fn find_asset_by_md5(md5: &str) -> Option<String> {
    let like = format!("%{}%", md5);
    if let Ok(blob) = DATABASE.lock_and_select("SELECT card FROM cards WHERE card LIKE ?1", params!(like.clone())) {
        if let Ok(card) = jzon::parse(&blob) {
            let master_card_id = card["master_card_id"].as_i64()?;
            for art in card["art"].members() {
                if art["md5"].as_str() == Some(md5) {
                    return Some(format!("{}/{}_{}.png", master_card_id, art["kind"], art["variant"]));
                }
            }
        }
    }
    let blob = DATABASE.lock_and_select("SELECT character FROM characters WHERE character LIKE ?1", params!(like)).ok()?;
    let character = jzon::parse(&blob).ok()?;
    let id = character["master_character_id"].as_i64()?;
    for art in character["art"].members() {
        if art["md5"].as_str() == Some(md5) {
            return Some(format!("characters/{}/{}.png", id, art["kind"]));
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use jzon::object;

    fn card_blob(id: i64, rarity: i64) -> JsonValue {
        object!{
            "master_card_id": id,
            "master_character_id": 1001,
            "rarity": rarity,
            "art": [{ "kind": "c", "variant": "00", "md5": format!("{:032x}", id), "size": 1 }]
        }
    }

    fn wipe(owner: i64) {
        for card in get_cards_by_owner(owner).members() {
            delete_card(card["master_card_id"].as_i64().unwrap());
        }
        for character in get_characters_by_owner(owner).members() {
            delete_character(character["master_character_id"].as_i64().unwrap());
        }
    }

    // Ids are sequential from FIRST_CARD_ID, never reused after a delete, and
    // never land on a seq-0 prefix boundary
    #[test]
    fn ids_are_sequential_and_never_reused() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(3001);

        let first = next_card_id();
        assert!(first >= FIRST_CARD_ID);
        assert_ne!(first % 10000, 0);
        insert_card(first, 1001, 3001, &card_blob(first, 1), false, false);
        let second = next_card_id();
        assert_eq!(second, first + 1);
        insert_card(second, 1001, 3001, &card_blob(second, 1), false, false);
        delete_card(second);
        assert_eq!(get_card(second), None);
        // The high-water mark survives the delete, so the id is retired
        assert_eq!(next_card_id(), second + 1);

        let character = next_character_id();
        assert!(character >= FIRST_CHARACTER_ID);
        insert_character(character, 3001, &object!{ "master_character_id": character });
        assert_eq!(next_character_id(), character + 1);
        delete_character(character);
        assert_eq!(next_character_id(), character + 1);

        wipe(3001);
        assert!(next_card_id() > second);
    }

    // Published cards go to everyone, drafts only to their owner, a game
    // account that owns a card keeps seeing it even unpublished, and a
    // character rides along with any card the viewer is served
    #[test]
    fn catalog_filters_per_user() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(3003);
        wipe(3004);

        let character = next_character_id();
        insert_character(character, 3003, &object!{ "master_character_id": character });
        let published = next_card_id();
        let mut blob = card_blob(published, 3);
        blob["master_character_id"] = character.into();
        insert_card(published, character, 3003, &blob, true, true);
        let draft = next_card_id();
        insert_card(draft, character, 3003, &card_blob(draft, 1), false, false);

        let owner_view = get_cards_for_user(3003, &[]);
        assert_eq!(owner_view.len(), 2);
        let other_view = get_cards_for_user(3004, &[]);
        assert_eq!(other_view.len(), 1);
        assert_eq!(other_view[0]["master_card_id"].as_i64(), Some(published));
        // The wire `obtainable` field comes from the column
        assert_eq!(other_view[0]["obtainable"].as_bool(), Some(true));

        // A player who owns the draft in game still resolves it
        let holder_view = get_cards_for_user(3004, &[draft]);
        assert_eq!(holder_view.len(), 2);

        // Unpublishing removes it from strangers, not from holders
        set_published(published, false);
        assert!(get_cards_for_user(3004, &[]).is_empty());
        assert_eq!(get_cards_for_user(3004, &[published]).len(), 1);
        set_published(published, true);

        // Characters follow the served cards; a stranger with no visible card
        // on the character doesn't get it
        let characters = get_characters_for_cards(3004, &get_cards_for_user(3004, &[]));
        assert_eq!(characters.len(), 1);
        assert_eq!(characters[0]["master_character_id"].as_i64(), Some(character));
        set_published(published, false);
        assert!(get_characters_for_cards(3004, &get_cards_for_user(3004, &[])).is_empty());
        // The owner always sees their own character
        assert_eq!(get_characters_for_cards(3003, &get_cards_for_user(3003, &[])).len(), 1);
        set_published(published, true);

        assert_eq!(character_of(published), Some(character));
        assert_eq!(cards_using_character(character), 2);
        assert!(character_publicly_visible(character));
        assert_eq!(card_count_for_owner(3003), 2);

        wipe(3003);
        wipe(3004);
    }

    // The draw pool is exactly the published + obtainable cards of the rarity
    #[test]
    fn obtainable_pool_by_rarity() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(3005);

        let r1 = next_card_id();
        insert_card(r1, 1001, 3005, &card_blob(r1, 1), true, true);
        let r3 = next_card_id();
        insert_card(r3, 1001, 3005, &card_blob(r3, 3), true, true);
        let unpublished = next_card_id();
        insert_card(unpublished, 1001, 3005, &card_blob(unpublished, 1), false, true);
        let unobtainable = next_card_id();
        insert_card(unobtainable, 1001, 3005, &card_blob(unobtainable, 1), true, false);

        assert_eq!(obtainable_card_ids(1), vec![r1]);
        assert_eq!(obtainable_card_ids(3), vec![r3]);
        assert!(obtainable_card_ids(2).is_empty());
        set_obtainable(unobtainable, true);
        assert_eq!(obtainable_card_ids(1), vec![r1, unobtainable]);

        wipe(3005);
    }

    #[test]
    fn md5_resolves_to_the_file_holding_the_bytes() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(3007);

        let id = next_card_id();
        insert_card(id, 1001, 3007, &card_blob(id, 1), true, false);
        assert_eq!(find_asset_by_md5(&format!("{:032x}", id)), Some(format!("{}/c_00.png", id)));
        assert_eq!(find_asset_by_md5("00000000000000000000000000000000"), None);

        let character = next_character_id();
        insert_character(character, 3007, &object!{
            "master_character_id": character,
            "art": [{ "kind": "icon", "md5": "aabbccddeeff00112233445566778899", "size": 1 }]
        });
        assert_eq!(
            find_asset_by_md5("aabbccddeeff00112233445566778899"),
            Some(format!("characters/{}/icon.png", character))
        );

        wipe(3007);
    }

    // Deleted ids come back from dead_card_ids; unpublished, imported and
    // official ids never do
    #[test]
    fn dead_ids_are_deleted_ids_only() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(3008);

        let alive = next_card_id();
        insert_card(alive, 1001, 3008, &card_blob(alive, 1), false, false);
        let dead = next_card_id();
        insert_card(dead, 1001, 3008, &card_blob(dead, 1), true, false);
        delete_card(dead);

        let dead_ids = dead_card_ids(&array![alive, dead, 10010001, 100010001, dead]);
        assert_eq!(dead_ids.len(), 1);
        assert_eq!(dead_ids[0].as_i64(), Some(dead));

        wipe(3008);
    }
}

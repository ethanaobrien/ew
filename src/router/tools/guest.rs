use jzon::{array, object, JsonValue};
use lazy_static::lazy_static;
use std::collections::HashMap;
use crate::router::userdata;
use crate::router::global;
use crate::router::{card, custom_card, databases};

fn get_clear_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for current in user["live_list"].members() {
        if current["level"] == level {
            rv += 1;
        }
    }
    rv
}

fn get_full_combo_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for current in user["live_mission_list"].members() {
        if current["clear_master_live_mission_ids"].contains(20 + level) {
            rv += 1;
        }
    }
    rv
}

fn get_perfect_count(user: &JsonValue, level: i32) -> i64 {
    let mut rv = 0;
    for current in user["live_mission_list"].members() {
        if current["clear_master_live_mission_ids"].contains(40 + level) {
            rv += 1;
        }
    }
    rv
}

fn get_high_score_rate(user: &JsonValue) -> JsonValue {
    let mut entries = vec![];
    for live in user["live_list"].members() {
        let rate = live["high_score"].as_i64().unwrap_or(0) / 5000;
        entries.push((rate, live["master_live_id"].as_i64().unwrap_or(0), live["level"].as_i64().unwrap_or(0)));
    }
    entries.sort_by(|a, b| b.0.cmp(&a.0));
    entries.truncate(10);

    let mut detail = array![];
    let mut total = 0;
    for (rate, master_live_id, level) in entries {
        total += rate;
        detail.push(object!{
            master_live_id: master_live_id,
            level: level,
            rate: rate
        }).unwrap();
    }

    object!{
        rate: total,
        detail: detail
    }
}

#[derive(Clone, Copy)]
pub enum UserView {
    Card,
    Detail,
    Ranking,
}

const DEFAULT_CARD: i64 = 10010001;

lazy_static! {
    // Each character's lowest official card id: the stand-in shown to viewers
    // who can't resolve a custom card of that character
    static ref OFFICIAL_CARD_BY_CHARACTER: HashMap<i64, i64> = {
        let mut rv: HashMap<i64, i64> = HashMap::new();
        for entry in databases::CARD_LIST.entries() {
            let Some(id) = entry.1["id"].as_i64() else { continue; };
            if card::is_custom(id) {
                continue;
            }
            let Some(character) = entry.1["masterCharacterId"].as_i64() else { continue; };
            let slot = rv.entry(character).or_insert(id);
            if id < *slot {
                *slot = id;
            }
        }
        rv
    };
}

// The character behind any card id: baked masterdata first, then the runtime
// custom-card db, then the imported band's id arithmetic (prefix - 9000)
fn card_character(id: i64) -> Option<i64> {
    let card = &databases::CARD_LIST[id.to_string()];
    if !card.is_empty() {
        return card["masterCharacterId"].as_i64();
    }
    if custom_card::is_custom_runtime(id) {
        return custom_card::character_of(id);
    }
    Some(id / 10000 - 9000)
}

// A custom card the viewer can't resolve shows as its character's base
// official card - the right face, never a crash. Characters with no official
// card (custom ones included) fall back to the default
fn proxy_card_id(id: i64) -> i64 {
    if !card::is_custom(id) {
        return id;
    }
    let Some(character) = card_character(id) else {
        return DEFAULT_CARD;
    };
    let Some(rv) = OFFICIAL_CARD_BY_CHARACTER.get(&character) else {
        return DEFAULT_CARD;
    };
    *rv
}

pub fn proxy_user_cards(user: &mut JsonValue, protocol: u32) {
    for key in ["favorite_master_card_id", "guest_smile_master_card_id", "guest_cool_master_card_id", "guest_pure_master_card_id"] {
        let id = user["user"][key].as_i64().unwrap_or(0);
        if !custom_card::viewer_can_resolve(id, protocol) {
            user["user"][key] = proxy_card_id(id).into();
        }
    }
    // A card's id is its master_card_id, so both sides need proxying
    for key in ["favorite_card", "guest_smile_card", "guest_cool_card", "guest_pure_card"] {
        let id = user[key]["master_card_id"].as_i64().unwrap_or(0);
        if !custom_card::viewer_can_resolve(id, protocol) {
            user[key]["id"] = proxy_card_id(id).into();
            user[key]["master_card_id"] = proxy_card_id(id).into();
        }
    }
    if !user["main_deck_detail"].is_empty() {
        let mut used = array![];
        for id in user["main_deck_detail"]["deck"]["main_card_ids"].members_mut() {
            let raw = id.as_i64().unwrap_or(0);
            let card = if custom_card::viewer_can_resolve(raw, protocol) { raw } else { proxy_card_id(raw) };
            // Whole characters share one proxy, and the client can't hold the
            // same card twice
            if card == 0 || used.contains(card) {
                *id = (0).into();
                continue;
            }
            used.push(card).unwrap();
            *id = card.into();
        }
        let mut cards = array![];
        let mut ids = array![];
        for card in user["main_deck_detail"]["card_list"].members() {
            let id = card["master_card_id"].as_i64().unwrap_or(0);
            let proxy = if custom_card::viewer_can_resolve(id, protocol) { id } else { proxy_card_id(id) };
            if ids.contains(proxy) {
                continue;
            }
            ids.push(proxy).unwrap();
            let mut card = card.clone();
            if proxy != id {
                card["id"] = proxy.into();
                card["master_card_id"] = proxy.into();
            }
            cards.push(card).unwrap();
        }
        user["main_deck_detail"]["card_list"] = cards;
    }
}

pub fn get_user(id: i64, friends: &JsonValue, view: UserView, protocol: u32) -> JsonValue {
    let user = userdata::get_acc_from_uid(id);
    if !user["error"].is_empty() {
        return object!{};
    }

    let mut rv = object!{
        user: user["user"].clone(),
        favorite_card: global::get_card(user["user"]["favorite_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_smile_card: global::get_card(user["user"]["guest_smile_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_cool_card: global::get_card(user["user"]["guest_cool_master_card_id"].as_i64().unwrap_or(0), &user),
        guest_pure_card: global::get_card(user["user"]["guest_pure_master_card_id"].as_i64().unwrap_or(0), &user)
    };

    if let UserView::Detail | UserView::Ranking = view {
        rv["main_deck_detail"] = object!{
            total_power: 0,
            deck: user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1].clone(),
            card_list: global::get_cards(user["deck_list"][user["user"]["main_deck_slot"].as_usize().unwrap_or(1) - 1]["main_card_ids"].clone(), &user)
        };
        rv["master_title_ids"] = user["user"]["master_title_ids"].clone();
    }

    if let UserView::Detail = view {
        rv["live_data_summary"] = object!{
            clear_count_list: [get_clear_count(&user, 1), get_clear_count(&user, 2), get_clear_count(&user, 3), get_clear_count(&user, 4)],
            full_combo_list: [get_full_combo_count(&user, 1), get_full_combo_count(&user, 2), get_full_combo_count(&user, 3), get_full_combo_count(&user, 4)],
            all_perfect_list: [get_perfect_count(&user, 1), get_perfect_count(&user, 2), get_perfect_count(&user, 3), get_perfect_count(&user, 4)],
            high_score_rate: get_high_score_rate(&user)
        };
    }

    rv["user"].remove("sif_user_id");
    rv["user"].remove("ss_user_id");
    rv["user"].remove("birthday");

    if let UserView::Card | UserView::Ranking = view {
        if !friends.is_empty() {
            rv["status"] = if friends["friend_user_id_list"].contains(id) {
                3
            } else if friends["pending_user_id_list"].contains(id) {
                2
            } else if friends["request_user_id_list"].contains(id) {
                1
            } else {
                0
            }.into();
        }
    }

    proxy_user_cards(&mut rv, protocol);

    rv
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::database::custom_card as db;

    fn wipe(owner: i64) {
        for card in db::get_cards_by_owner(owner).members() {
            db::delete_card(card["master_card_id"].as_i64().unwrap());
        }
        for character in db::get_characters_by_owner(owner).members() {
            db::delete_character(character["master_character_id"].as_i64().unwrap());
        }
    }

    // The imported band proxies to its own character's base card, not to a
    // single default (the old prefix arithmetic collapsed 14000+ to Honoka)
    #[test]
    fn proxies_resolve_through_the_character() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(5101);

        assert_eq!(proxy_card_id(10010001), 10010001);
        assert_eq!(proxy_card_id(0), 0);
        assert_eq!(proxy_card_id(100010001), 10010001);
        assert_eq!(proxy_card_id(100090001), 10090001);
        assert_eq!(proxy_card_id(110010001), 20010001);
        assert_eq!(proxy_card_id(130010001), 40010001);
        assert_eq!(proxy_card_id(140010001), DEFAULT_CARD);

        // A runtime card on an official character proxies to that character
        let id = db::next_card_id();
        db::insert_card(id, 2003, 5101, &jzon::object!{ "master_card_id": id, "rarity": 1 }, true, false);
        assert_eq!(proxy_card_id(id), 20030001);
        // On a custom character (no official card) it falls to the default
        let orphan = db::next_card_id();
        db::insert_card(orphan, db::FIRST_CHARACTER_ID, 5101, &jzon::object!{ "master_card_id": orphan, "rarity": 1 }, true, false);
        assert_eq!(proxy_card_id(orphan), DEFAULT_CARD);
        // A deleted/unknown runtime id can't resolve a character either
        let unknown = db::next_card_id() + 5000;
        assert_eq!(proxy_card_id(unknown), DEFAULT_CARD);
        // A proxy is never zero and always a real row
        for id in [100010001i64, 141720001, 149990001, id, orphan, unknown] {
            let proxy = proxy_card_id(id);
            assert_ne!(proxy, 0, "id {}", id);
            assert!(!databases::CARD_LIST[proxy.to_string()].is_empty(), "id {}", id);
        }

        wipe(5101);
    }

    // Even a protocol-3 viewer gets a proxy for a draft - published is what
    // makes a runtime card resolvable, and older viewers proxy everything
    #[test]
    fn drafts_and_old_viewers_get_proxies() {
        let _lock = crate::runtime::lock_test_data_path();
        wipe(5102);

        let published = db::next_card_id();
        db::insert_card(published, 2003, 5102, &jzon::object!{ "master_card_id": published, "rarity": 1 }, true, false);
        let draft = db::next_card_id();
        db::insert_card(draft, 2003, 5102, &jzon::object!{ "master_card_id": draft, "rarity": 1 }, false, false);

        assert!(custom_card::viewer_can_resolve(published, custom_card::PROTOCOL_VERSION));
        assert!(!custom_card::viewer_can_resolve(draft, custom_card::PROTOCOL_VERSION));

        let mut user = jzon::object!{
            "user": {
                "favorite_master_card_id": draft,
                "guest_smile_master_card_id": published
            },
            "favorite_card": { "id": draft, "master_card_id": draft },
            "guest_smile_card": { "id": published, "master_card_id": published },
            "main_deck_detail": {
                "deck": { "main_card_ids": [draft, published, 10010001, 0, 0] },
                "card_list": [
                    { "id": draft, "master_card_id": draft },
                    { "id": published, "master_card_id": published },
                    { "id": 10010001, "master_card_id": 10010001 }
                ]
            }
        };
        proxy_user_cards(&mut user, custom_card::PROTOCOL_VERSION);
        assert_eq!(user["user"]["favorite_master_card_id"].as_i64(), Some(20030001));
        assert_eq!(user["user"]["guest_smile_master_card_id"].as_i64(), Some(published));
        assert_eq!(user["favorite_card"]["master_card_id"].as_i64(), Some(20030001));
        assert_eq!(user["guest_smile_card"]["master_card_id"].as_i64(), Some(published));
        assert_eq!(user["main_deck_detail"]["deck"]["main_card_ids"][0].as_i64(), Some(20030001));
        assert_eq!(user["main_deck_detail"]["deck"]["main_card_ids"][1].as_i64(), Some(published));
        assert_eq!(user["main_deck_detail"]["deck"]["main_card_ids"][2].as_i64(), Some(10010001));
        assert_eq!(user["main_deck_detail"]["card_list"].len(), 3);
        for card in user["main_deck_detail"]["card_list"].members() {
            assert_ne!(card["master_card_id"].as_i64(), Some(draft));
        }

        // Protocol 0-2 viewers proxy even the published runtime card
        for protocol in [0u32, 1, 2] {
            let mut user = jzon::object!{
                "user": { "favorite_master_card_id": published },
                "favorite_card": { "id": published, "master_card_id": published }
            };
            proxy_user_cards(&mut user, protocol);
            assert_eq!(user["user"]["favorite_master_card_id"].as_i64(), Some(20030001), "protocol {}", protocol);
        }
        // The baked import band needs only protocol 2
        let mut user = jzon::object!{
            "user": { "favorite_master_card_id": 100010001 },
            "favorite_card": { "id": 100010001, "master_card_id": 100010001 }
        };
        proxy_user_cards(&mut user, 2);
        assert_eq!(user["user"]["favorite_master_card_id"].as_i64(), Some(100010001));
        proxy_user_cards(&mut user, 1);
        assert_eq!(user["user"]["favorite_master_card_id"].as_i64(), Some(10010001));

        wipe(5102);
    }
}

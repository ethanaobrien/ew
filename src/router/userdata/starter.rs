// Accounts that are born finished.
//
// Everything below Title in the client assumes a tutorial-complete account: a
// name, a favourite card, a nine-card deck, tutorial_step 130. The tutorial that
// produces one is four client requests, and nothing on the server can create
// that state on its own - which is exactly what the arcade needs, twice per
// cabinet (the machine identity and its guest) and again at every credit (the
// guest, rewritten in place).
//
// So this module replays those four requests' mutations, in their order, off
// their own handlers' code:
//
//   POST /api/lottery            the tutorial draw leaves a card in card_list,
//                                which is what /user/initialize reads as the
//                                account's "ur" (lottery.rs:342-358, user.rs:383)
//   POST /api/user/initialize    favourite + guest cards, 3000 gems, the band
//                                title, the nine base cards, deck slot 1,
//                                character_list, the first bond mission
//                                (user.rs:371-453)
//   POST /api/user               the player's name (user.rs:138-140)
//   POST /api/tutorial           tutorial_step 130 and full stamina
//                                (tutorial.rs:13-17)
//
// The one deliberate difference is the gacha: a cabinet draws nothing random.
// The chosen member's own base card takes the "ur" slot instead, which leaves
// the starter deck complete (nine distinct cards) rather than one short, and
// makes every arcade account byte-identical apart from its name.
//
// `write_starter_rows` is the single source of truth for what an account is made
// of, shared by creation and by the guest reset - the two can never drift.
//
// It is also the single source of truth for what an account is NOT made of. A
// guest is handed to a stranger every credit, so the reset has to leave behind
// exactly what a freshly created account has: the eleven data rows, one login
// token, and nothing else. Every other row `delete_account` recognises as
// account-owned (mod.rs:791) is deleted here rather than rewritten - the
// transfer code and password /api/user/registerpassword registers, the webui
// session, the gree device certificate - because each of them is a credential
// the previous player could keep and come back with. The login token is the one
// thing that is kept in the sense that the account keeps having one, but it is
// re-drawn too: the old one is on the cabinet's disk and may be on the previous
// player's phone, and /api/arcade/session hands the new one straight back to the
// cabinet in the `uuid` the client already adopts.

use jzon::{array, object, JsonValue};
use rusqlite::params;

use crate::include_file;
use crate::router::{card, chat, global, items, live};
use super::{DATABASE, NEW_USER, acc_exists, generate_uid};

// The member every arcade account is built around: Kousaka Honoka, the first
// member of the first band, so the deck is deterministic and recognisable.
// user/initialize derives everything else from this one id.
const STARTER_CHARACTER_ID: i64 = 1001;

// The nine cards /user/initialize rewards for a mu's pick (user.rs:398)
const STARTER_CARDS: &[i64] = &[
    10010001, 10020001, 10030001, 10040001, 10050001,
    10060001, 10070001, 10080001, 10090001
];

// The card the tutorial gacha would have left in card_list[0]. See the module
// note: the chosen member's own base card, so the deck comes out whole.
const STARTER_UR: i64 = 10010001;

// user.rs:396-411: 3000000 + the band offset (0 for mu's) + the member's index
// within the band, which is the last two digits of the character id
const STARTER_TITLE_ID: i64 = 3_000_000 + STARTER_CHARACTER_ID % 100;

// Every account name is stored verbatim by /api/user, so a cabinet name is no
// more dangerous than a player name - but it arrives from a machine that types
// it once and never again, so it is clamped rather than trusted to be sane.
pub const MAX_NAME_LEN: usize = 32;

pub fn clean_name(name: &str, fallback: &str) -> String {
    let name: String = name
        .chars()
        .filter(|c| !c.is_control())
        .take(MAX_NAME_LEN)
        .collect();
    let name = name.trim();
    if name.is_empty() {
        return fallback.to_string();
    }
    name.to_string()
}

// The userdata blob a finished tutorial leaves behind, plus the two side rows it
// writes on the way (the mission progress and the chat it unlocks).
fn tutorial_complete(uid: i64, name: &str) -> (JsonValue, JsonValue, JsonValue, JsonValue) {
    let now = global::timestamp();

    // create_acc (mod.rs:230-232)
    let mut user = NEW_USER.clone();
    user["user"]["id"] = uid.into();
    user["stamina"]["last_updated_time"] = now.into();

    let mut home = jzon::parse(&include_file!("src/router/userdata/new_user_home.json")).unwrap();
    let mut missions = jzon::parse(&include_file!("src/router/userdata/missions.json")).unwrap();
    let mut chats = array![];

    // --- POST /api/user/initialize (user.rs:371-453) ------------------------
    chat::add_chat(STARTER_CHARACTER_ID, 1, &mut chats);

    user["user"]["favorite_master_card_id"] = STARTER_UR.into();
    user["user"]["guest_smile_master_card_id"] = STARTER_UR.into();
    user["user"]["guest_cool_master_card_id"] = STARTER_UR.into();
    user["user"]["guest_pure_master_card_id"] = STARTER_UR.into();
    home["home"]["preset_setting"][0]["illust_master_card_id"] = STARTER_UR.into();
    user["gem"]["free"] = (3000).into();
    user["gem"]["total"] = (3000).into();
    user["user"]["master_title_ids"][0] = STARTER_TITLE_ID.into();

    // The clear-mission and chat out-parameters are thrown away here exactly as
    // /user/initialize throws them away (user.rs:418): the only chat a fresh
    // account keeps is the one added above.
    for id in STARTER_CARDS {
        items::give_character(*id, &mut user, &mut missions, &mut array![], &mut array![]);
    }

    let mut others = array![];
    for id in STARTER_CARDS {
        if id / 10000 != STARTER_CHARACTER_ID {
            others.push(*id).unwrap();
        }
    }
    for slot in 0..9 {
        let card_id = if slot == 4 {
            STARTER_UR
        } else if slot < 4 {
            others[slot].as_i64().unwrap_or(0)
        } else {
            others[slot - 1].as_i64().unwrap_or(0)
        };
        user["deck_list"][0]["main_card_ids"][slot] = card_id.into();
    }

    user["character_list"] = array![object!{
        master_character_id: STARTER_CHARACTER_ID,
        exp: 1
    }];
    let bond = live::bond_missions(STARTER_CHARACTER_ID);
    if !bond.is_empty() {
        items::advance_mission(bond[0][1].as_i64().unwrap(), 1, bond[0][0].as_i64().unwrap(), &mut missions);
    }

    // --- POST /api/user (user.rs:138-140) -----------------------------------
    user["user"]["name"] = name.into();

    // --- POST /api/tutorial, the final step (tutorial.rs:13-17) -------------
    user["tutorial_step"] = (130).into();
    user["stamina"]["stamina"] = (100).into();
    user["stamina"]["last_updated_time"] = now.into();

    (user, home, missions, chats)
}

// Everything create_acc seeds, as (table, value). The column always carries the
// table's own name; `userdata` is the one row with extra columns and is written
// by the caller.
fn side_rows(chats: &JsonValue, home: &JsonValue, missions: &JsonValue) -> Vec<(&'static str, String)> {
    let now = global::timestamp();
    vec![
        ("userhome", jzon::stringify(home.clone())),
        ("missions", jzon::stringify(missions.clone())),
        ("chats", jzon::stringify(chats.clone())),
        ("loginbonus", format!(r#"{{"last_rewarded": 0, "bonus_list": [], "start_time": {}}}"#, now)),
        ("eventloginbonus", format!(r#"{{"last_rewarded": 0, "bonus_list": [], "start_time": {}}}"#, now)),
        ("sifcards", String::from("[]")),
        ("friends", String::from(r#"{"friend_user_id_list":[],"request_user_id_list":[],"pending_user_id_list":[]}"#)),
        ("event", String::from("{}")),
        ("exchange", String::from("[]")),
        ("server_data", format!(r#"{{"server_time_set":{},"server_time":1709272800}}"#, now))
    ]
}

// The rows delete_account (mod.rs:791) removes that write_starter_rows does not
// write back: everything an account owns that is a credential rather than
// progress. `tokens` is not here because the starter write issues a fresh one in
// the same transaction, and `userdata` and its ten side tables are not here
// because they are rewritten. Anything added to delete_account's list belongs in
// one of those three places or the guest reset starts leaking again.
const CARRIED_CREDENTIAL_TABLES: &[&str] = &["migration", "webui"];

// Writes the whole account inside one transaction: the eleven data rows back to
// the starter state, every carried-over credential gone, and a login token
// nobody has seen before. Upserts rather than inserts, so the same code both
// creates an account and rewrites an existing one - the reset can never
// half-apply, and it cannot miss a table that creation seeds because there is
// only one list. Returns the account's new login token.
fn write_starter_rows(uid: i64, name: &str) -> Result<String, rusqlite::Error> {
    let (user, home, missions, chats) = tutorial_complete(uid, name);
    let rows = side_rows(&chats, &home, &missions);
    let friend_request_disabled = user["user"]["friend_request_disabled"].as_i32().unwrap_or(1);
    let protocol_version = if card::account_has_custom_cards(&user) { card::PROTOCOL_VERSION } else { 0 };
    let userdata = jzon::stringify(user);
    let token = global::create_token();

    DATABASE.lock_and_transact(|conn| {
        conn.execute(
            "INSERT INTO userdata (user_id, userdata, friend_request_disabled, protocol_version) VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(user_id) DO UPDATE SET userdata=?2, friend_request_disabled=?3, protocol_version=?4",
            params!(uid, &userdata, friend_request_disabled, protocol_version)
        )?;
        for (table, value) in &rows {
            conn.execute(
                &format!(
                    "INSERT INTO {0} (user_id, {0}) VALUES (?1, ?2) ON CONFLICT(user_id) DO UPDATE SET {0}=?2",
                    table
                ),
                params!(uid, value)
            )?;
        }
        for table in CARRIED_CREDENTIAL_TABLES {
            conn.execute(&format!("DELETE FROM {} WHERE user_id=?1", table), params!(uid))?;
        }
        // The account's one login token, re-drawn. The DELETE by token is
        // create_acc's own collision guard (mod.rs:238); the DELETE by user id is
        // what makes the INSERT a rotation rather than a primary-key conflict.
        conn.execute("DELETE FROM tokens WHERE token=?1", params!(&token))?;
        conn.execute("DELETE FROM tokens WHERE user_id=?1", params!(uid))?;
        conn.execute("INSERT INTO tokens (user_id, token) VALUES (?1, ?2)", params!(uid, &token))?;
        Ok(token)
    })
}

// A brand new account, already through the tutorial. Returns its user id and its
// login token - the uuid the client stores and authenticates with from then on.
pub fn create(name: &str) -> Option<(i64, String)> {
    let uid = generate_uid();
    match write_starter_rows(uid, name) {
        Ok(token) => Some((uid, token)),
        Err(err) => {
            println!("arcade: could not create account {}: {}", uid, err);
            None
        }
    }
}

// Rewrites an existing account back to the starter state, keeping its user id:
// the cabinet's guest, at the start of every credit. Returns the account's new
// login token, which /api/arcade/session answers with - the previous player's
// copy of the old one stops working the moment their credit ends.
pub fn reset(uid: i64, name: &str) -> Option<String> {
    if !acc_exists(uid) {
        return None;
    }
    let token = match write_starter_rows(uid, name) {
        Ok(token) => token,
        Err(err) => {
            println!("arcade: could not reset account {}: {}", uid, err);
            return None;
        }
    };
    // The gree device certificate is the one account-owned credential that lives
    // in another database, so it cannot ride the transaction above - but it is on
    // delete_account's list for the same reason the two tables are, and a stale
    // one would still name this account (database/gree.rs:98).
    crate::database::gree::delete_uuid(uid);
    Some(token)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::router::userdata;

    fn stored(uid: i64) -> JsonValue {
        jzon::parse(&DATABASE.lock_and_select("SELECT userdata FROM userdata WHERE user_id=?1", params!(uid)).unwrap()).unwrap()
    }

    fn row(uid: i64, table: &str) -> String {
        DATABASE.lock_and_select(&format!("SELECT {0} FROM {0} WHERE user_id=?1", table), params!(uid)).unwrap()
    }

    // A created account is already past the tutorial: named, nine cards, a full
    // deck with the favourite in the centre, gems, title, bond and full stamina
    #[test]
    fn a_created_account_is_tutorial_complete() {
        let _lock = crate::runtime::lock_test_data_path();

        let (uid, token) = create("Cabinet 1").unwrap();
        assert_eq!(userdata::uid_from_login_token(&token), uid);

        let user = stored(uid);
        assert_eq!(user["user"]["id"].as_i64(), Some(uid));
        assert_eq!(user["user"]["name"].as_str(), Some("Cabinet 1"));
        // 130 is what every gate downstream tests for (live.rs:92, :385, :435)
        assert_eq!(user["tutorial_step"].as_i64(), Some(130));
        assert_eq!(user["stamina"]["stamina"].as_i64(), Some(100));
        assert_eq!(user["gem"]["free"].as_i64(), Some(3000));
        assert_eq!(user["gem"]["total"].as_i64(), Some(3000));
        assert_eq!(user["user"]["master_title_ids"][0].as_i64(), Some(3000001));
        assert_eq!(user["user"]["favorite_master_card_id"].as_i64(), Some(STARTER_UR));
        assert_eq!(user["character_list"][0]["master_character_id"].as_i64(), Some(STARTER_CHARACTER_ID));

        assert_eq!(user["card_list"].len(), STARTER_CARDS.len());
        for id in STARTER_CARDS {
            assert!(user["card_list"].members().any(|c| c["master_card_id"] == *id), "missing {}", id);
        }

        // The deck is whole: nine distinct cards, the favourite in the centre
        let deck = &user["deck_list"][0]["main_card_ids"];
        assert_eq!(deck.len(), 9);
        assert_eq!(deck[4].as_i64(), Some(STARTER_UR));
        let mut seen = Vec::new();
        for slot in deck.members() {
            let id = slot.as_i64().unwrap();
            assert!(id != 0, "empty deck slot");
            assert!(!seen.contains(&id), "duplicate card {} in the deck", id);
            seen.push(id);
        }

        // The home row carries the favourite too, and every side row exists
        assert_eq!(jzon::parse(&row(uid, "userhome")).unwrap()["home"]["preset_setting"][0]["illust_master_card_id"].as_i64(), Some(STARTER_UR));
        assert_eq!(row(uid, "sifcards"), "[]");
        assert_eq!(row(uid, "exchange"), "[]");
        assert_eq!(row(uid, "event"), "{}");
        assert!(!jzon::parse(&row(uid, "missions")).unwrap().is_empty());
        assert!(!jzon::parse(&row(uid, "chats")).unwrap().is_empty());

        // The account is NOT one of the dead ones the purge sweeper collects
        // (mod.rs:792-801): it has cards, a real name and step 130
        assert!(!user["card_list"].is_empty());
        assert_ne!(user["user"]["name"].as_str(), Some("Tutorial in progress"));
    }

    // The guest reset keeps the identity and the token and throws away
    // everything the last player did with it
    #[test]
    fn a_reset_keeps_the_identity_and_wipes_the_progress() {
        let _lock = crate::runtime::lock_test_data_path();

        let (uid, token) = create("GUEST").unwrap();

        // A credit's worth of play, on every row a reset has to reach
        let mut user = stored(uid);
        user["user"]["name"] = "Somebody".into();
        user["user"]["exp"] = (12345).into();
        user["stamina"]["stamina"] = (3).into();
        user["gem"]["free"] = (99999).into();
        user["live_list"].push(object!{ master_live_id: 1100101, level: 4, clear_count: 7, high_score: 654321, max_combo: 300 }).unwrap();
        user["live_mission_list"].push(object!{ master_live_id: 1100101, clear_master_live_mission_ids: [1, 2] }).unwrap();
        user["item_list"].push(object!{ id: 17001001, master_item_id: 17001001, amount: 50 }).unwrap();
        user["card_list"].push(object!{ id: 10010013, master_card_id: 10010013, exp: 0, skill_exp: 0, evolve: [], created_date_time: 0 }).unwrap();
        userdata::save_acc(&token, user);
        userdata::save_acc_friends(&token, object!{
            friend_user_id_list: [1],
            request_user_id_list: [],
            pending_user_id_list: []
        });
        userdata::save_server_data(&token, object!{ last_live_started: [object!{ master_live_id: 1100101 }] });
        userdata::save_acc_exchange(&token, array![1, 2, 3]);

        let new_token = reset(uid, "Cabinet 1").expect("the reset was refused");

        // Same account, a new login token - the cabinet is handed the new one in
        // the /api/arcade/session answer, and the previous player's copy of the
        // old one now names nothing
        assert_ne!(new_token, token, "the guest's login token was reused");
        assert_eq!(userdata::uid_from_login_token(&new_token), uid);
        assert_eq!(userdata::uid_from_login_token(&token), 0, "the previous credit's token still logs in");

        let user = stored(uid);
        assert_eq!(user["user"]["id"].as_i64(), Some(uid));
        assert_eq!(user["user"]["name"].as_str(), Some("Cabinet 1"));
        assert_eq!(user["user"]["exp"].as_i64(), Some(0));
        assert_eq!(user["stamina"]["stamina"].as_i64(), Some(100));
        assert_eq!(user["gem"]["free"].as_i64(), Some(3000));
        assert_eq!(user["live_list"].len(), 0);
        assert_eq!(user["live_mission_list"].len(), 0);
        assert_eq!(user["item_list"].len(), 0);
        assert_eq!(user["card_list"].len(), STARTER_CARDS.len());
        assert_eq!(user["deck_list"][0]["main_card_ids"][4].as_i64(), Some(STARTER_UR));

        // Every side row went back too
        assert_eq!(jzon::parse(&row(uid, "friends")).unwrap()["friend_user_id_list"].len(), 0);
        assert!(jzon::parse(&row(uid, "server_data")).unwrap()["last_live_started"].is_null());
        assert_eq!(row(uid, "exchange"), "[]");
    }

    // Resetting something that is not an account is refused rather than
    // conjuring one out of nothing
    #[test]
    fn resetting_an_unknown_account_is_refused() {
        let _lock = crate::runtime::lock_test_data_path();
        assert!(reset(123, "Cabinet 1").is_none());
    }

    // A guest is handed to a stranger every credit, so a reset has to take every
    // credential the last player could have left on it - not just the progress.
    // The list is delete_account's (mod.rs:791) minus the rows the starter write
    // rewrites and the token it re-draws.
    #[test]
    fn a_reset_takes_every_credential_the_last_player_left() {
        let _lock = crate::runtime::lock_test_data_path();

        let (uid, token) = create("GUEST").unwrap();

        // The previous player, on the cabinet during their credit: a transfer
        // code and password (/api/user/registerpassword), a webui session logged
        // in with them, and a gree device certificate.
        let code = userdata::user::migration::save_acc_transfer(uid, "hunter2");
        assert!(userdata::has_transfer_password(uid));
        let webui_token = userdata::webui_login(uid, "hunter2").expect("the webui login was refused");
        assert_eq!(userdata::webui_login_token(&webui_token).as_deref(), Some(token.as_str()));
        crate::database::gree::update_cert(uid, "a device certificate");
        assert!(crate::database::gree::get_user_cert(&token).is_some());

        let new_token = reset(uid, "Cabinet 1").expect("the reset was refused");

        // The transfer code and password are gone: neither the game's own
        // takeover check nor the webui login answers to them any more
        assert!(!userdata::has_transfer_password(uid), "the transfer password survived the reset");
        assert!(!userdata::user::migration::get_acc_transfer(&code, "hunter2")["success"].as_bool().unwrap_or(false),
            "the previous player's transfer code still takes the account over");
        assert!(userdata::webui_login(uid, "hunter2").is_err(), "the previous player can still log the webui in");

        // The webui session they left open is gone too
        assert!(userdata::webui_login_token(&webui_token).is_none(), "the previous player's webui session survived the reset");
        assert!(userdata::webui_get_user(&webui_token).is_none());

        // So is the device certificate, which named the account from a phone
        assert!(crate::database::gree::get_user_cert(&token).is_none(), "the gree certificate survived the reset");
        assert!(crate::database::gree::get_user_cert(&new_token).is_none());

        // And the account is still perfectly usable under its new token
        assert_eq!(userdata::uid_from_login_token(&new_token), uid);
        assert_eq!(stored(uid)["user"]["name"].as_str(), Some("Cabinet 1"));
    }

    // Every table delete_account clears is either rewritten by the starter write,
    // re-drawn (the token) or deleted by it. A row added to delete_account without
    // a decision here is a leak across credits, so the lists are compared rather
    // than trusted to have been kept in step.
    #[test]
    fn the_reset_accounts_for_every_table_a_deletion_clears() {
        let rewritten = ["userdata", "userhome", "missions", "chats", "loginbonus",
            "eventloginbonus", "sifcards", "friends", "event", "exchange", "server_data"];
        for table in userdata::ACCOUNT_TABLES {
            assert!(
                rewritten.contains(table) || CARRIED_CREDENTIAL_TABLES.contains(table) || *table == "tokens",
                "delete_account clears {} and the guest reset does nothing about it",
                table
            );
        }
        // And the side-row list really is what the reset rewrites
        let rows = side_rows(&array![], &object!{}, &object!{});
        for (table, _) in &rows {
            assert!(rewritten.contains(table), "{} is written by the reset but not listed above", table);
        }
        assert_eq!(rows.len() + 1, rewritten.len());
    }

    // Cabinet names arrive from an operator typing into a machine
    #[test]
    fn names_are_clamped_not_trusted() {
        assert_eq!(clean_name("  Cabinet 1  ", "ARCADE"), "Cabinet 1");
        assert_eq!(clean_name("", "ARCADE"), "ARCADE");
        assert_eq!(clean_name("   ", "ARCADE"), "ARCADE");
        assert_eq!(clean_name("a\nb\tc", "ARCADE"), "abc");
        assert_eq!(clean_name(&"x".repeat(200), "ARCADE").len(), MAX_NAME_LEN);
    }
}

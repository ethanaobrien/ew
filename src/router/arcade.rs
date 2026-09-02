// Arcade mode: a SIF2 client turned into a rhythm cabinet.

use jzon::{object, JsonValue};
use actix_web::{web, Responder};

use crate::router::{databases, global, live, multi_live, userdata, Api, Body};
use crate::router::userdata::user::migration;
use crate::database::arcade as database;

const DEFAULT_MACHINE_NAME: &str = "ARCADE";
const GUEST_NAME: &str = "GUEST";

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/arcade")
            .route("/info", web::get().to(info))
            .route("/register", web::post().to(register))
            .route("/session", web::post().to(session))
            .route("/bind", web::post().to(bind))
    );
}

pub fn disabled() -> bool {
    let args = crate::get_args();
    args.hidden || !args.enable_arcade
}

// 0 = forever
fn machine_ttl_days() -> u64 {
    crate::get_args().arcade_machine_ttl
}

fn session_ttl() -> i64 {
    crate::get_args().arcade_session_ttl as i64 * 60
}

const MAX_SESSION_WINDOWS: i64 = 4;

fn flag(value: &JsonValue) -> bool {
    value.as_bool().unwrap_or(false) || value.as_i64().unwrap_or(0) != 0
}

fn card_id(body: &JsonValue) -> Option<String> {
    migration::valid_card_id(body["card_id"].as_str().unwrap_or(""))
}

pub fn is_cabinet_account(user_id: i64) -> bool {
    !disabled() && database::machine_of_account(user_id).is_some()
}

pub fn card_relinked(card: &str) {
    if !disabled() {
        database::clear_card_session(card);
    }
}

fn live_card_session(user_id: i64, now: i64) -> Option<(String, i64)> {
    database::live_card_session(&migration::cards_of_account(user_id), now)
}

fn open_card_session(card: &str, machine_id: &str) {
    database::open_card_session(card, machine_id, global::timestamp() as i64 + session_ttl());
}

async fn info() -> impl Responder {
    if disabled() {
        return Api(None);
    }
    Api(Some(object!{
        "enabled": true,
        "machine_ttl_days": machine_ttl_days()
    }))
}

async fn register(Body(body): Body) -> impl Responder {
    if disabled() {
        return Api(None);
    }
    let name = userdata::starter::clean_name(body["name"].as_str().unwrap_or(""), DEFAULT_MACHINE_NAME);

    let Some((machine_user_id, machine_uuid)) = userdata::starter::create(&name) else {
        return Api(None);
    };
    let Some((guest_user_id, guest_uuid)) = userdata::starter::create(GUEST_NAME) else {
        userdata::delete_account(machine_user_id);
        return Api(None);
    };

    let machine_id = database::generate_machine_id();
    database::insert_machine(&machine_id, &name, machine_user_id, guest_user_id);
    println!("arcade: registered machine {} \"{}\" (machine {}, guest {})", machine_id, name, machine_user_id, guest_user_id);

    Api(Some(object!{
        "machine_id": machine_id,
        "machine_user_id": machine_user_id,
        "machine_uuid": machine_uuid,
        "guest_user_id": guest_user_id,
        "guest_uuid": guest_uuid
    }))
}

fn resolve_card(card: &str) -> Option<(i64, String)> {
    let user_id = migration::card_user(card)?;
    let uuid = userdata::get_login_token(user_id);
    if uuid.is_empty() {
        println!("arcade: card mapping pointed at missing account {} - unlinking the card", user_id);
        migration::remove_card(card);
        return None;
    }
    Some((user_id, uuid))
}

async fn session(Body(body): Body) -> impl Responder {
    if disabled() {
        return Api(None);
    }
    let machine_id = body["machine_id"].as_str().unwrap_or("").to_string();
    let Some(machine) = database::get_machine(&machine_id) else {
        println!("arcade: session for unknown machine \"{}\"", machine_id);
        return Api(None);
    };
    database::touch_machine(&machine_id);
    let machine_name = machine["name"].as_str().unwrap_or(DEFAULT_MACHINE_NAME).to_string();

    let presented = !body["card_id"].is_null() && !body["card_id"].as_str().unwrap_or("").trim().is_empty();
    let card = card_id(&body);
    if presented && card.is_none() {
        println!("arcade: machine {} presented an unusable card id", machine_id);
        return Api(None);
    }

    let Some(card) = card else {
        let guest_user_id = machine["guest_user_id"].as_i64().unwrap_or(0);
        let Some(uuid) = userdata::starter::reset(guest_user_id, &machine_name) else {
            println!("arcade: machine {} has no guest account to reset", machine_id);
            return Api(None);
        };
        return Api(Some(object!{
            "user_id": guest_user_id,
            "uuid": uuid,
            "name": machine_name,
            "unlinked": false
        }));
    };

    // A known card plays its own account, with every clear it has ever earned
    let Some((user_id, uuid)) = resolve_card(&card) else {
        println!("arcade: machine {} presented a card that is linked to no account", machine_id);
        return Api(Some(object!{ "unlinked": true }));
    };
    open_card_session(&card, &machine_id);
    let name = userdata::get_name_and_rank(user_id)["user_name"].as_str().unwrap_or("").to_string();
    Api(Some(object!{
        "user_id": user_id,
        "uuid": uuid,
        "name": name,
        "unlinked": false
    }))
}

pub fn bind_card_to(card: &str, user_id: i64) -> Result<i64, String> {
    if disabled() {
        return Err(String::from("Arcade mode is disabled on this server"));
    }
    migration::link_card(card, user_id)?;
    println!("arcade: card {} now plays as account {}", card, user_id);
    Ok(user_id)
}

pub fn bind_card(card: &str, migration_code: &str, pass: &str) -> Result<i64, String> {
    if disabled() {
        return Err(String::from("Arcade mode is disabled on this server"));
    }
    let account = userdata::user::migration::get_acc_transfer(migration_code, pass);
    if !account["success"].as_bool().unwrap_or(false) || account["user_id"] == 0 {
        return Err(String::from("Transfer code and password don't match"));
    }
    let Some(user_id) = account["user_id"].as_i64() else {
        return Err(String::from("Transfer code and password don't match"));
    };
    bind_card_to(card, user_id)
}

async fn bind(Body(body): Body) -> impl Responder {
    match bind_card(
        body["card_id"].as_str().unwrap_or(""),
        &body["migrationCode"].to_string(),
        &body["pass"].to_string()
    ) {
        Ok(user_id) => Api(Some(object!{ "user_id": user_id })),
        Err(reason) => {
            println!("arcade: bind refused - {}", reason);
            Api(None)
        }
    }
}

// lives

fn cabinet_account_at(login_token: &str, now: i64) -> Option<i64> {
    let user_id = userdata::uid_from_login_token(login_token);
    if user_id == 0 {
        return None;
    }
    if database::machine_of_account(user_id).is_some() {
        return Some(user_id);
    }
    live_card_session(user_id, now).map(|_| user_id)
}

fn arcade_account_at(login_token: &str, body: &JsonValue, now: i64) -> Option<i64> {
    if !flag(&body["arcade"]) || disabled() {
        return None;
    }
    cabinet_account_at(login_token, now)
}

pub fn arcade_play_user(login_token: &str, body: &JsonValue) -> Option<i64> {
    let user_id = arcade_account_at(login_token, body, global::timestamp() as i64)?;
    let Some(started) = live::get_started_live(login_token, body) else {
        println!("arcade: account {} ended an arcade live that was never started", user_id);
        return None;
    };
    if !flag(&started["arcade"]) {
        println!("arcade: account {} ended a live it started as an ordinary play", user_id);
        return None;
    }
    Some(user_id)
}

fn play_machine(user_id: i64) -> Option<String> {
    if let Some(machine) = database::machine_of_account(user_id) {
        return machine["machine_id"].as_str().map(str::to_string);
    }
    database::last_machine_of_cards(&migration::cards_of_account(user_id))
}

pub fn live_started(login_token: &str, body: &JsonValue) {
    let now = global::timestamp() as i64;
    let Some(user_id) = arcade_account_at(login_token, body, now) else { return; };
    if let Some(machine_id) = play_machine(user_id) {
        database::touch_machine(&machine_id);
    }
    if let Some((card, opened)) = live_card_session(user_id, now) {
        let ttl = session_ttl();
        database::extend_card_session(&card, (now + ttl).min(opened + ttl * MAX_SESSION_WINDOWS));
    }
}

pub fn live_end_body(body: &JsonValue) -> JsonValue {
    let mut rv = body.clone();
    rv["use_lp"] = multi_live::boost_lp(1).into();
    rv
}

fn score_rank(live_id: i64, score: i64) -> i64 {
    let live = &databases::LIVE_LIST[live_id.to_string()];
    let mut rank = 0;
    for (index, key) in ["scoreC", "scoreB", "scoreA", "scoreS"].iter().enumerate() {
        if let Some(threshold) = live[*key].as_i64()
            && threshold > 0
            && score >= threshold {
            rank = index as i64 + 1;
        }
    }
    rank
}

pub fn arcade_retire_user(login_token: &str, body: &JsonValue) -> Option<i64> {
    let user_id = retire_user_at(login_token, body, global::timestamp() as i64)?;
    if disabled() {
        return None;
    }
    Some(user_id)
}

fn retire_user_at(login_token: &str, body: &JsonValue, now: i64) -> Option<i64> {
    let started = live::get_started_live(login_token, body)?;
    if !flag(&started["arcade"]) {
        return None;
    }
    cabinet_account_at(login_token, now)
}

pub fn record_play(user_id: i64, body: &JsonValue, cleared: bool) {
    let Some(machine_id) = play_machine(user_id) else { return; };
    let live_id = body["master_live_id"].as_i64().unwrap_or(0);
    let level = body["level"].as_i64().unwrap_or(0);
    let score = body["live_score"]["score"].as_i64().unwrap_or(0);
    database::insert_play(&machine_id, user_id, live_id, level, score, score_rank(live_id, score), cleared);
    database::touch_machine(&machine_id);
}

// maintenance

pub fn purge_machines() -> usize {
    if disabled() {
        return 0;
    }
    let ttl = machine_ttl_days();
    // 0 = never
    if ttl == 0 {
        return 0;
    }
    purge_machines_before(global::timestamp() as i64 - (ttl as i64 * 24 * 60 * 60))
}

fn purge_machines_before(cutoff: i64) -> usize {
    let dead = database::machines_last_seen_before(cutoff);
    for machine in dead.members() {
        let machine_id = machine["machine_id"].as_str().unwrap_or("");
        println!(
            "Removing arcade machine {} (last seen {})",
            machine_id,
            global::format_datetime(machine["last_seen"].as_u64().unwrap_or(0))
        );
        for key in ["machine_user_id", "guest_user_id"] {
            let user_id = machine[key].as_i64().unwrap_or(0);
            if user_id != 0 && !migration::account_has_card(user_id) {
                userdata::delete_account(user_id);
            }
        }
        database::delete_machine(machine_id);
    }
    dead.len()
}

// webui

pub fn webui_machines() -> JsonValue {
    if disabled() {
        return jzon::array![];
    }
    database::list_machines()
}

pub fn webui_remove_machine(machine_id: &str) -> Result<(), String> {
    if disabled() {
        return Err(String::from("Arcade mode is disabled on this server"));
    }
    let Some(machine) = database::get_machine(machine_id) else {
        return Err(format!("No arcade machine {}", machine_id));
    };
    for key in ["machine_user_id", "guest_user_id"] {
        let user_id = machine[key].as_i64().unwrap_or(0);
        if user_id != 0 && !migration::account_has_card(user_id) {
            userdata::delete_account(user_id);
        }
    }
    database::delete_machine(machine_id);
    println!("arcade: machine {} removed through the webui", machine_id);
    Ok(())
}


// Rest of file is tests

#[cfg(test)]
mod tests {
    use super::*;

    // The flag the client sends as 0/1 and the flag it might send as a bool are
    // the same flag; nothing else is
    #[test]
    fn the_arcade_flag_reads_both_shapes() {
        assert!(flag(&true.into()));
        assert!(flag(&(1).into()));
        assert!(!flag(&false.into()));
        assert!(!flag(&(0).into()));
        assert!(!flag(&JsonValue::Null));
        assert!(!flag(&"yes".into()));
    }

    // A card id is refused rather than repaired
    #[test]
    fn card_ids_are_validated_not_sanitised() {
        assert_eq!(card_id(&object!{ card_id: "0123456789012345" }).as_deref(), Some("0123456789012345"));
        assert_eq!(card_id(&object!{ card_id: " 0123456789012345 " }).as_deref(), Some("0123456789012345"));
        assert!(card_id(&object!{}).is_none());
        assert!(card_id(&object!{ card_id: "" }).is_none());
        assert!(card_id(&object!{ card_id: "0123-4567" }).is_none());
        assert!(card_id(&object!{ card_id: "'; DROP TABLE cards--" }).is_none());
    }

    // The rank stored in the ledger is the one the result screen shows, read off
    // the live's own masterdata thresholds (live.csv 1100101: C 20000, B 100000,
    // A 250000, S 350000)
    #[test]
    fn the_ledger_rank_comes_from_the_lives_own_thresholds() {
        assert_eq!(score_rank(1100101, 0), 0);
        assert_eq!(score_rank(1100101, 19_999), 0);
        assert_eq!(score_rank(1100101, 20_000), 1);
        assert_eq!(score_rank(1100101, 100_000), 2);
        assert_eq!(score_rank(1100101, 250_000), 3);
        assert_eq!(score_rank(1100101, 350_000), 4);
        assert_eq!(score_rank(1100101, 9_999_999), 4);
        // A custom song has no official row, so it has no rank
        assert_eq!(score_rank(10_000, 9_999_999), 0);
    }

    // A card resolves to the account it names, and to nothing at all once that
    // account is gone - and the dead mapping does not survive the answer
    #[test]
    fn a_card_resolves_to_its_account_and_a_dead_mapping_is_dropped() {
        let _lock = crate::runtime::lock_test_data_path();
        use crate::database::arcade as db;

        // Nobody linked it: unlinked, and nothing was created for it
        assert!(resolve_card("7020392000000001").is_none());
        assert!(migration::card_user("7020392000000001").is_none());

        // Linked: the account and its token
        let (player, token) = userdata::starter::create("Linked").unwrap();
        migration::set_card("7020392000000002", player);
        assert_eq!(resolve_card("7020392000000002"), Some((player, token)));

        // The account was deleted: unlinked from now on, mapping gone
        userdata::delete_account(player);
        assert!(resolve_card("7020392000000002").is_none());
        assert!(migration::card_user("7020392000000002").is_none(), "a mapping to a deleted account survived");
    }

    // bind_card_to is the cabinet's entrance to the shared link rule
    // (migration::link_card): a cabinet's own identities are refused and a card
    // that already names an account is re-pointed, never refused
    #[test]
    fn bind_card_to_refuses_cabinet_identities_and_repoints_a_linked_card() {
        let _lock = crate::runtime::lock_test_data_path();
        use crate::database::arcade as db;

        let (machine_account, _) = userdata::starter::create("Cabinet Bind").unwrap();
        let (guest_account, _) = userdata::starter::create(GUEST_NAME).unwrap();
        let machine_id = db::generate_machine_id();
        db::insert_machine(&machine_id, "Cabinet Bind", machine_account, guest_account);
        let (player, _) = userdata::starter::create("Player").unwrap();

        // The id is validated, not repaired
        assert!(bind_card_to("0123-4567", player).is_err());
        assert!(migration::card_user("0123-4567").is_none());

        // Neither cabinet identity may sit behind a card
        let card = "4242424242424242";
        assert!(bind_card_to(card, machine_account).is_err());
        assert!(bind_card_to(card, guest_account).is_err());
        assert!(migration::card_user(card).is_none());

        // A card another player linked is re-pointed, and that player's account
        // is untouched - the card was the only thing that changed hands
        let (previous, _) = userdata::starter::create("Previous").unwrap();
        migration::set_card(card, previous);
        assert_eq!(bind_card_to(card, player), Ok(player));
        assert_eq!(migration::card_user(card), Some(player));
        assert!(!userdata::get_acc_from_uid(previous)["error"].as_bool().unwrap_or(false), "re-pointing a card touched the previous account");

        db::delete_machine(&machine_id);
        for id in [player, previous, machine_account, guest_account] {
            userdata::delete_account(id);
        }
    }

    // A cabinet's song that ended with an empty life gauge is reported at
    // /live/retire, which carries no arcade flag - the start record is what
    // proves it was a cabinet's - and it lands in the ledger as a failed song
    // rather than not at all.
    #[test]
    fn a_failed_cabinet_song_lands_in_the_ledger_uncleared() {
        let _lock = crate::runtime::lock_test_data_path();
        use crate::database::arcade as db;

        let (machine_account, _) = userdata::starter::create("Cabinet Retire").unwrap();
        let (guest_account, guest_token) = userdata::starter::create(GUEST_NAME).unwrap();
        let machine_id = db::generate_machine_id();
        db::insert_machine(&machine_id, "Cabinet Retire", machine_account, guest_account);

        // The credit's song: /live/start carried the flag, so the record does
        let start = object!{
            master_live_id: 1100101,
            level: 4,
            deck_slot: 1,
            live_boost: 1,
            arcade: true
        };
        live::start_live(&guest_token, &start);

        // The life gauge emptied: the client played it out and reported the score
        // it reached at /live/retire, which has no arcade flag of its own
        let retire = object!{
            master_live_id: 1100101,
            level: 4,
            live_score: { score: 123_456, max_combo: 40, play_time: 93 }
        };
        let now = global::timestamp() as i64;
        let user_id = retire_user_at(&guest_token, &retire, now).expect("a cabinet's retire was not recognised");
        assert_eq!(user_id, guest_account);
        record_play(user_id, &retire, false);

        let ledger = db::plays_of_machine(&machine_id);
        assert_eq!(ledger.len(), 1);
        assert_eq!(ledger[0]["cleared"].as_bool(), Some(false));
        assert_eq!(ledger[0]["user_id"].as_i64(), Some(guest_account));
        assert_eq!(ledger[0]["live_id"].as_i64(), Some(1100101));
        assert_eq!(ledger[0]["level"].as_i64(), Some(4));
        assert_eq!(ledger[0]["score"].as_i64(), Some(123_456), "the retire's own score was not carried");
        // It counts as a song of the credit, and only the cleared count excludes it
        assert!(db::list_machines().members().any(|m|
            m["machine_id"] == machine_id.as_str() && m["play_count"] == 1 && m["cleared_count"] == 0
        ));

        // A live that was started as an ordinary play is not a cabinet's song,
        // however the retire that ends it looks
        live::start_live(&guest_token, &object!{ master_live_id: 1100102, level: 4, deck_slot: 1 });
        assert!(retire_user_at(&guest_token, &object!{
            master_live_id: 1100102,
            level: 4,
            live_score: { score: 1, max_combo: 1, play_time: 93 }
        }, now).is_none(), "an unflagged start was bookkept as a cabinet's song");

        // Neither is a retire with no start behind it at all
        assert!(retire_user_at(&guest_token, &object!{
            master_live_id: 1100103,
            level: 4,
            live_score: { score: 1, max_combo: 1, play_time: 93 }
        }, now).is_none());

        db::delete_machine(&machine_id);
        userdata::delete_account(machine_account);
        userdata::delete_account(guest_account);
    }

    // A cabinet that aged out takes its two accounts with it - and nothing
    // else. A cabinet still in use, and any account a player bound a card to,
    // survives the sweep.
    #[test]
    fn an_aged_out_cabinet_takes_only_its_own_accounts() {
        let _lock = crate::runtime::lock_test_data_path();
        use crate::database::arcade as db;

        // Old: unseen since before the cutoff
        let (old_machine, old_machine_token) = userdata::starter::create("Old cabinet").unwrap();
        let (old_guest, old_guest_token) = userdata::starter::create(GUEST_NAME).unwrap();
        let old_id = db::generate_machine_id();
        db::insert_machine(&old_id, "Old cabinet", old_machine, old_guest);

        // Live: seen just now
        let (live_machine, live_machine_token) = userdata::starter::create("Live cabinet").unwrap();
        let (live_guest, _) = userdata::starter::create(GUEST_NAME).unwrap();
        let live_id = db::generate_machine_id();
        db::insert_machine(&live_id, "Live cabinet", live_machine, live_guest);

        // Old too, but somebody bound a card to its machine account
        let (claimed_machine, claimed_machine_token) = userdata::starter::create("Claimed cabinet").unwrap();
        let (claimed_guest, _) = userdata::starter::create(GUEST_NAME).unwrap();
        let claimed_id = db::generate_machine_id();
        db::insert_machine(&claimed_id, "Claimed cabinet", claimed_machine, claimed_guest);
        migration::set_card("4444333322221111", claimed_machine);

        // A player account with a card, belonging to no cabinet at all
        let (player, player_token) = userdata::starter::create("Player").unwrap();
        migration::set_card("8888777766665555", player);

        // Everything registered above is "seen now"; only the two we push back
        // are older than the cutoff
        let now = global::timestamp() as i64;
        for id in [&old_id, &claimed_id] {
            db::backdate_machine_for_test(id, now - 1000);
        }

        assert_eq!(purge_machines_before(now - 500), 2);

        // The aged-out cabinet is gone, accounts and all
        assert!(db::get_machine(&old_id).is_none());
        assert_eq!(userdata::uid_from_login_token(&old_machine_token), 0);
        assert_eq!(userdata::uid_from_login_token(&old_guest_token), 0);

        // The cabinet still in use is untouched
        assert!(db::get_machine(&live_id).is_some());
        assert_eq!(userdata::uid_from_login_token(&live_machine_token), live_machine);

        // The claimed cabinet is retired, but the account behind its card is not
        assert!(db::get_machine(&claimed_id).is_none());
        assert_eq!(userdata::uid_from_login_token(&claimed_machine_token), claimed_machine);
        assert_eq!(migration::card_user("4444333322221111"), Some(claimed_machine));

        // A card-bound player account is never a candidate in the first place
        assert_eq!(userdata::uid_from_login_token(&player_token), player);

        db::delete_machine(&live_id);
    }
}

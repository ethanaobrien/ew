// Arcade mode: a SIF2 client turned into a rhythm cabinet.
//
// A cabinet registers once and gets a machine id plus two accounts it owns
// forever - the MACHINE account (the identity the attract loop and its demo
// lives run on) and one reusable GUEST. Every credit calls /session; without a
// card that rewrites the guest back to the starter state in place, keeping its
// user id but re-drawing everything a player could have left on it - its login
// token included - so the machine plays a clean account and the server never
// accumulates dead users. With a card, the card names the account and the play
// is real progress on it.
//
// Credits replace LP: a live flagged `arcade` runs through live_end_ex with
// consume_lp = false - the seam /multi_live/end already uses - and its use_lp
// pinned to one normal play, so rewards, EXP, bonds, high scores and clears all
// record exactly as they do on a phone while LP is never touched.
//
// That flag is money, so it is not taken on trust. It buys a free play only for
// a machine's own two identities, or for a card account inside the window the
// credit at /api/arcade/session opened for it, and only when the /live/start
// this /live/end answers was itself flagged. See arcade_account_at.
//
// The whole feature is opt-in (--enable-arcade) and additionally off in
// --hidden mode. When disabled every endpoint answers like the custom-song
// endpoints do with their flag off - Api(None), as if it never existed - and
// nothing touches arcade.db, so no table setup runs.

use jzon::{object, JsonValue};
use actix_web::{web, Responder};

use crate::router::{databases, global, live, multi_live, userdata, Api, Body};
use crate::database::arcade as database;

// The name a cabinet falls back to when its operator sent nothing usable
const DEFAULT_MACHINE_NAME: &str = "ARCADE";

// Guest accounts are created under this name and renamed to their cabinet's own
// name at the first session (design 4.3: a credit plays as the machine)
const GUEST_NAME: &str = "GUEST";

// NESiCA ids are 16 ASCII digits; the cap is generous enough for any other
// reader an I/O provider might present without letting an id become a blob
const MAX_CARD_ID_LEN: usize = 32;

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

// Days a machine may go unseen before --purge deletes it. 0 means never.
fn machine_ttl_days() -> u64 {
    crate::get_args().arcade_machine_ttl
}

// How long one credit buys LP-free play for the card that paid it. Long enough
// that a slow credit never runs out mid-song (the design's play is two songs),
// short enough that a card left on a reader overnight is not an open tap.
fn session_ttl() -> i64 {
    crate::get_args().arcade_session_ttl as i64 * 60
}

// A live played inside the window pushes it back, so a credit that runs long is
// never cut off - but a credit is a credit, and past this many windows from the
// session that opened it the extension stops. Without a ceiling one tap of a
// card would buy LP-free lives for as long as the player kept starting them.
const MAX_SESSION_WINDOWS: i64 = 4;

// The client writes its request-body flags as 0/1 ints (auto_play, is_omakase,
// ...), so `arcade` is read as either that or a JSON bool.
fn flag(value: &JsonValue) -> bool {
    value.as_bool().unwrap_or(false) || value.as_i64().unwrap_or(0) != 0
}

// A card id is an identifier, never text: it is refused rather than sanitised,
// because a mangled id would silently name a different card's account.
fn card_id(body: &JsonValue) -> Option<String> {
    let card_id = body["card_id"].as_str().unwrap_or("").trim().to_string();
    if card_id.is_empty() {
        return None;
    }
    if card_id.len() > MAX_CARD_ID_LEN || !card_id.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    Some(card_id)
}

// The credit is taken: this card is at this cabinet, and for the next ttl it
// plays the way a cabinet plays. The single place a window is ever opened -
// /api/arcade/session is the one moment the server is told a credit was spent.
fn open_card_session(card: &str, machine_id: &str) {
    database::open_card_session(card, machine_id, global::timestamp() as i64 + session_ttl());
}

// An account made for a card nobody has named yet takes the card's last four
// digits, the way a cabinet prints them on a receipt. The on-cabinet name entry
// overwrites it.
fn card_account_name(card_id: &str) -> String {
    let start = card_id.len().saturating_sub(4);
    card_id[start..].to_string()
}

// The account this card was issued seconds ago, and which nothing has happened
// to since - the only account /api/arcade/session will ever rename.
//
// The cabinet's name entry is a second /session for the same card: the first one
// created the account and answered is_new, the player typed a name, and this is
// the same call again carrying it. By then the card is KNOWN, so the rename has
// to be told apart from an ordinary card session, and it has to be told apart
// hard: a rename that reached a real account would let anyone holding a card id
// retitle a stranger's save. Every clause below has to hold.
//
// Deliberately not a `renamed` column: the account itself carries the proof.
fn issued_by_this_credit(card: &str, user_id: i64, now: i64, ttl: i64) -> bool {
    // The credit that created it is still running, at this card. live_card_session
    // is the window /api/arcade/session opened and nothing else ever opens
    // (database/arcade.rs:240), so this is "the same credit, still going". `ttl`
    // is session_ttl() in production and the test's own number in the test - the
    // shape purge_machines_before and retire_user_at already use here.
    let Some((open_card, opened)) = database::live_card_session(user_id, now) else {
        return false;
    };
    if open_card != card || now - opened > ttl {
        return false;
    }
    // A cabinet's own machine or guest identity is never a card's account
    if database::machine_of_account(user_id).is_some() {
        return false;
    }
    // A phone has owned it - the same test bind_card's cleanup trusts
    if userdata::has_transfer_password(user_id) {
        return false;
    }
    let user = userdata::get_acc_from_uid(user_id);
    if user["error"].as_bool().unwrap_or(false) {
        return false;
    }
    // It still carries the placeholder /session gave it, and nothing has been
    // played on it. Either alone would be enough to make a rename harmless; both
    // together make it provable.
    if user["user"]["name"].as_str().unwrap_or("") != card_account_name(card) {
        return false;
    }
    user["live_list"].is_empty() && user["live_mission_list"].is_empty()
}

// The one write a rename is. /api/user does exactly this for a phone player
// (user.rs:138-140); there is no other name in an account.
fn rename_account(user_id: i64, login_token: &str, name: &str) {
    let mut user = userdata::get_acc_from_uid(user_id);
    user["user"]["name"] = name.into();
    userdata::save_acc(login_token, user);
}

// -- endpoints --------------------------------------------------------------

// The client asks this before it offers to convert a device: a server without
// the module answers None and the Title-menu entry refuses.
async fn info() -> impl Responder {
    if disabled() {
        return Api(None);
    }
    Api(Some(object!{
        "enabled": true,
        "machine_ttl_days": machine_ttl_days()
    }))
}

// Converting a fresh device into a cabinet. Pre-login by nature: the device has
// no account yet, which is exactly the state the Title-menu entry requires.
async fn register(Body(body): Body) -> impl Responder {
    if disabled() {
        return Api(None);
    }
    let name = userdata::starter::clean_name(body["name"].as_str().unwrap_or(""), DEFAULT_MACHINE_NAME);

    let Some((machine_user_id, machine_uuid)) = userdata::starter::create(&name) else {
        return Api(None);
    };
    let Some((guest_user_id, guest_uuid)) = userdata::starter::create(GUEST_NAME) else {
        // Half a cabinet is worse than none: the machine account has nothing
        // pointing at it and nobody holding its token, so it goes back.
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

// One credit. Answers the account the player is about to become: the cabinet's
// guest (reset in place) or, with a card, the account that card names.
//
// `name` is the cabinet's on-screen name entry, and it is read in exactly two
// places: the account a brand new card is issued, and a rename of that same
// account while the credit that created it is still running (see
// issued_by_this_credit). A guest credit ignores it - a guest is named after its
// cabinet - and so does an ordinary card session.
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

    // A guest credit is a session with no card_id at all (every credit in v1) or
    // an empty one. Anything else is a card being presented, and a card id that
    // does not parse is refused rather than quietly played as a guest on
    // somebody's credit.
    let presented = !body["card_id"].is_null() && !body["card_id"].as_str().unwrap_or("").trim().is_empty();
    let card = card_id(&body);
    if presented && card.is_none() {
        println!("arcade: machine {} presented an unusable card id", machine_id);
        return Api(None);
    }
    let typed_name = body["name"].as_str().unwrap_or("").trim().to_string();

    let Some(card) = card else {
        // The cabinet's own guest, rewritten from scratch. Same user id, so the
        // machine keeps its one guest forever - but a new login token, because
        // the previous player had a credit's worth of time alone with the old
        // one. The client adopts the uuid this answer carries (ArcadeEntranceScene
        // OnSessionResponse -> MngArcadeData.AdoptIdentity), so the rotation is
        // invisible to it.
        let guest_user_id = machine["guest_user_id"].as_i64().unwrap_or(0);
        let Some(uuid) = userdata::starter::reset(guest_user_id, &machine_name) else {
            println!("arcade: machine {} has no guest account to reset", machine_id);
            return Api(None);
        };
        return Api(Some(object!{
            "user_id": guest_user_id,
            "uuid": uuid,
            "name": machine_name,
            "is_new": true
        }));
    };

    // A known card plays its own account, with every clear it has ever earned
    if let Some(user_id) = database::card_user(&card) {
        let uuid = userdata::get_login_token(user_id);
        if !uuid.is_empty() {
            // The cabinet's name entry, coming back for the account this credit
            // just made. Checked BEFORE the window is re-opened, because the
            // proof is the window the previous call opened.
            if !typed_name.is_empty() && issued_by_this_credit(&card, user_id, global::timestamp() as i64, session_ttl()) {
                let named = userdata::starter::clean_name(&typed_name, &card_account_name(&card));
                rename_account(user_id, &uuid, &named);
                println!("arcade: machine {} named its new card's account {} \"{}\"", machine_id, user_id, named);
            }
            open_card_session(&card, &machine_id);
            let name = userdata::get_name_and_rank(user_id)["user_name"].as_str().unwrap_or("").to_string();
            return Api(Some(object!{
                "user_id": user_id,
                "uuid": uuid,
                "name": name,
                "is_new": false
            }));
        }
        // The mapping outlived its account (deleted through the webui, or aged
        // out with a machine). The card is not broken: it gets a new account.
        println!("arcade: card mapping pointed at missing account {} - reissuing", user_id);
    }

    // A card nobody has seen before. It gets an account named after its own last
    // four digits unless the cabinet already has a name for it - which it only
    // does when something other than the entrance's own two-step asked for one.
    let name = userdata::starter::clean_name(&typed_name, &card_account_name(&card));
    let Some((user_id, uuid)) = userdata::starter::create(&name) else {
        return Api(None);
    };
    database::set_card(&card, user_id);
    open_card_session(&card, &machine_id);
    println!("arcade: machine {} issued account {} to a new card", machine_id, user_id);

    Api(Some(object!{
        "user_id": user_id,
        "uuid": uuid,
        "name": name,
        "is_new": true
    }))
}

// Point a card at a player account the caller has already identified. This is
// the rule every entrance shares - the card id is validated, a cabinet's own
// identities are refused, the mapping is replaced and the throwaway account
// the card was carrying is cleaned up - and the proof of *whose* account it is
// belongs to the caller: the cabinet's /api/arcade/bind takes the game's
// data-transfer code and password (bind_card), the webui account page takes
// the signed-in session itself, which already proves the account.
pub fn bind_card_to(card: &str, user_id: i64) -> Result<i64, String> {
    if disabled() {
        return Err(String::from("Arcade mode is disabled on this server"));
    }
    let Some(card) = card_id(&object!{ "card_id": card }) else {
        return Err(String::from("That is not a usable card id"));
    };
    // A cabinet's own two identities are not player accounts and may never be
    // behind a card. The guest in particular is rewritten for a stranger every
    // credit: a card pointing at it would outlive that reset, and /session hands
    // out the account's current login token to whoever presents the card. Every
    // proof a bind can take - a transfer code and password, a webui login - is
    // exactly what a hostile client can register on a guest during its own
    // credit, so the refusal lives here, below all of them.
    if database::machine_of_account(user_id).is_some() {
        return Err(String::from("That account belongs to an arcade cabinet"));
    }

    let previous = database::card_user(&card);
    database::set_card(&card, user_id);

    // A card that was carrying a throwaway account the cabinet made for it takes
    // that account with it. Anything the player might care about keeps the card
    // from taking it: see orphan_of_a_rebound_card.
    if let Some(previous) = previous
        && previous != user_id
        && orphan_of_a_rebound_card(previous) {
        println!("arcade: card {} left empty account {} behind - removing", card, previous);
        userdata::delete_account(previous);
    }

    println!("arcade: card {} now plays as account {}", card, user_id);
    Ok(user_id)
}

// The cabinet's bind: the proof that a phone account is the player's is the
// game's own data-transfer code and password. On a phone the transfer runs
// through GREE's native code, and ew's /api/user/gglverifymigrationcode is only
// the desktop route with GGL off; what both share is the `migration` table -
// the code and the hashed password - which get_acc_transfer is the one owner
// of. A cabinet is a Windows build with GGL off, so that pair is its legitimate
// path, typed in from the game's Data Transfer screen.
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

// An account /api/arcade/session issued to an unknown card that was then never
// used for anything. Every one of these has to hold for it to be thrown away
// with the card, because a false positive deletes somebody's save:
//   * no other card still points at it,
//   * it is not a cabinet's own machine or guest identity,
//   * it has never registered a transfer password, so no phone ever owned it,
//   * and it has no live record at all - nothing was ever played on it.
fn orphan_of_a_rebound_card(user_id: i64) -> bool {
    if database::account_has_card(user_id) {
        return false;
    }
    if database::machine_of_account(user_id).is_some() {
        return false;
    }
    if userdata::has_transfer_password(user_id) {
        return false;
    }
    let user = userdata::get_acc_from_uid(user_id);
    if user["error"].as_bool().unwrap_or(false) {
        return false;
    }
    user["live_list"].is_empty() && user["live_mission_list"].is_empty()
}

// -- lives ------------------------------------------------------------------

// The account playing on a cabinet right now, None when the `arcade` flag is
// just a flag in a request body.
//
// The flag decides whether the play costs LP, so it is honoured for exactly two
// kinds of account:
//
//   * one of a machine's own two identities. The cabinet holds both tokens, the
//     guest is rewritten from scratch at every credit and the machine account
//     only ever runs the attract loop, so a free play on either buys nobody
//     anything.
//   * an account a card is bound to, and only while the credit that card paid
//     for is still running: /api/arcade/session opened a window on the card row
//     and it has not closed yet.
//
// A card mapping on its own proves nothing - a player can bind a card to their
// own account from the webui account page without ever standing in front of a
// cabinet - so an account whose card is not at a machine right now is an
// ordinary phone account and pays LP, exactly as before.
fn cabinet_account_at(login_token: &str, now: i64) -> Option<i64> {
    let user_id = userdata::uid_from_login_token(login_token);
    if user_id == 0 {
        return None;
    }
    if database::machine_of_account(user_id).is_some() {
        return Some(user_id);
    }
    database::live_card_session(user_id, now).map(|_| user_id)
}

// The same account rule behind the request's own opt-in flag, which /live/start
// and /live/end carry. The flag is read first so a phone play never reaches any
// of the arcade lookups, nor the module's own on/off switch.
fn arcade_account_at(login_token: &str, body: &JsonValue, now: i64) -> Option<i64> {
    if !flag(&body["arcade"]) || disabled() {
        return None;
    }
    cabinet_account_at(login_token, now)
}

// The account id when this really is an arcade play, None when the live should
// run the ordinary way.
//
// On top of the account rule above, /live/end has to agree with the /live/start
// it belongs to. start_live recorded the whole start body (live.rs:331), so the
// flag it carried is still there to read: a live that began as an ordinary play
// ends as one however its /live/end body is flagged, and a client cannot turn a
// finished LP-paid play into a free one after the fact.
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

// The cabinet a play is attributed to. A machine's own two accounts belong to
// it outright; a card account belongs to nobody, so it is credited to the
// machine that most recently ran a session for that card.
fn play_machine(user_id: i64) -> Option<String> {
    if let Some(machine) = database::machine_of_account(user_id) {
        return machine["machine_id"].as_str().map(str::to_string);
    }
    database::last_machine_of_card_account(user_id)
}

// A live starting on a cabinet is a sighting for it: last_seen is what the TTL
// sweeper measures, and a machine in daily use must never age out under it.
//
// It is also the credit saying it is still going. A song that runs long - or a
// second song of the same credit - pushes the card's window back so the play it
// is part of cannot expire underneath it, up to the ceiling above.
pub fn live_started(login_token: &str, body: &JsonValue) {
    let now = global::timestamp() as i64;
    let Some(user_id) = arcade_account_at(login_token, body, now) else { return; };
    if let Some(machine_id) = play_machine(user_id) {
        database::touch_machine(&machine_id);
    }
    if let Some((card, opened)) = database::live_card_session(user_id, now) {
        let ttl = session_ttl();
        database::extend_card_session(&card, (now + ttl).min(opened + ttl * MAX_SESSION_WINDOWS));
    }
}

// Credits already paid for the play, so use_lp is no longer what it costs - it
// is only what every reward scales off (live.rs:781). Pinned here to one normal
// 1x play rather than taken from the request: a cabinet that never spends LP
// must not be able to ask for a 10x payout.
pub fn live_end_body(body: &JsonValue) -> JsonValue {
    let mut rv = body.clone();
    rv["use_lp"] = multi_live::boost_lp(1).into();
    rv
}

// The result rank the client's own result screen shows, derived from the live's
// own thresholds - the same _scoreC/_scoreB/_scoreA/_scoreS columns the score
// missions read (live.rs:465). 4 = S, 3 = A, 2 = B, 1 = C; 0 is below C, and is
// also what a custom song gets, having no official thresholds.
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

// A cabinet's failed song. The retire wire carries no `arcade` flag - the
// client's CJsonSendParamLiveRetire is master_live_id, level and live_score and
// nothing else (Protocol.cs:7298-7305) - so the proof that this was a cabinet
// play is the start it belongs to: start_live recorded the whole /live/start
// body, and a cabinet's start is flagged. The account rule on top is /live/end's,
// unchanged.
//
// Read before live.rs's live_retire, which sweeps the very record this reads.
pub fn arcade_retire_user(login_token: &str, body: &JsonValue) -> Option<i64> {
    // The start record is read before the module's on/off switch so an ordinary
    // player's retire - whose start was never flagged - costs one lookup and
    // nothing else, exactly as it did before the ledger learned about failures.
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

// One row in the cabinet's ledger, and a sighting for it. Records nothing when
// the account has no cabinet to attribute the play to - a card bound through the
// webui that has never been to a machine still plays, it is just not anyone's
// bookkeeping.
//
// `cleared` is false for a song reported at /live/retire because its life gauge
// emptied. It is recorded whatever its play_time, unlike the global clear-rate
// counter live.rs:30 gates at five seconds: that gate keeps a rage-quit out of a
// public board, while a credit's song is a song of that credit either way. The
// score is the one the retire carried, and the rank is what that score is worth
// against the live's own thresholds - `cleared` is what tells the two apart.
pub fn record_play(user_id: i64, body: &JsonValue, cleared: bool) {
    let Some(machine_id) = play_machine(user_id) else { return; };
    let live_id = body["master_live_id"].as_i64().unwrap_or(0);
    let level = body["level"].as_i64().unwrap_or(0);
    let score = body["live_score"]["score"].as_i64().unwrap_or(0);
    database::insert_play(&machine_id, user_id, live_id, level, score, score_rank(live_id, score), cleared);
    database::touch_machine(&machine_id);
}

// -- maintenance ------------------------------------------------------------

// Machines unseen for --arcade-machine-ttl days, deleted with the two accounts
// each of them owns. Run from the --purge sweep at boot.
//
// A card-bound player account is never touched here, and neither is a machine
// or guest account somebody has bound a card to: cards outlive cabinets by
// design, and the account behind one is a player's.
pub fn purge_machines() -> usize {
    if disabled() {
        return 0;
    }
    let ttl = machine_ttl_days();
    // 0 is "never age a cabinet out", not "age every cabinet out this second"
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
            if user_id != 0 && !database::account_has_card(user_id) {
                userdata::delete_account(user_id);
            }
        }
        database::delete_machine(machine_id);
    }
    dead.len()
}

// -- webui ------------------------------------------------------------------

// The operator's machine list: name, id, last seen and how many lives the
// cabinet has recorded.
pub fn webui_machines() -> JsonValue {
    if disabled() {
        return jzon::array![];
    }
    database::list_machines()
}

// Retiring a cabinet by hand: the same deletion the TTL sweeper performs, with
// the same rule about accounts a card has claimed.
pub fn webui_remove_machine(machine_id: &str) -> Result<(), String> {
    if disabled() {
        return Err(String::from("Arcade mode is disabled on this server"));
    }
    let Some(machine) = database::get_machine(machine_id) else {
        return Err(format!("No arcade machine {}", machine_id));
    };
    for key in ["machine_user_id", "guest_user_id"] {
        let user_id = machine[key].as_i64().unwrap_or(0);
        if user_id != 0 && !database::account_has_card(user_id) {
            userdata::delete_account(user_id);
        }
    }
    database::delete_machine(machine_id);
    println!("arcade: machine {} removed through the webui", machine_id);
    Ok(())
}

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
        assert!(card_id(&object!{ card_id: "x".repeat(MAX_CARD_ID_LEN + 1) }).is_none());
    }

    // The account a fresh card gets is named after the card
    #[test]
    fn a_new_card_is_named_after_its_last_four_digits() {
        assert_eq!(card_account_name("0123456789012345"), "2345");
        assert_eq!(card_account_name("12"), "12");
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

    // The card-rebind cleanup only ever takes an account that is provably a
    // throwaway: a real player's account survives losing its card
    #[test]
    fn only_an_untouched_throwaway_follows_its_card() {
        let _lock = crate::runtime::lock_test_data_path();

        // Never played, no card, no password, not a cabinet: a throwaway
        let (orphan, _) = userdata::starter::create("2345").unwrap();
        assert!(orphan_of_a_rebound_card(orphan));

        // Somebody played on it
        let (played, played_token) = userdata::starter::create("2346").unwrap();
        let mut user = userdata::get_acc_from_uid(played);
        user["live_list"].push(object!{ master_live_id: 1100101, level: 4, clear_count: 1, high_score: 1, max_combo: 1 }).unwrap();
        userdata::save_acc(&played_token, user);
        assert!(!orphan_of_a_rebound_card(played));

        // Somebody can take it over from a phone
        let (phone, _) = userdata::starter::create("2347").unwrap();
        userdata::user::migration::save_acc_transfer(phone, "hunter2");
        assert!(!orphan_of_a_rebound_card(phone));

        // Another card still points at it
        let (shared, _) = userdata::starter::create("2348").unwrap();
        crate::database::arcade::set_card("9999888877776666", shared);
        assert!(!orphan_of_a_rebound_card(shared));

        // It is a cabinet's own identity
        let (machine_account, _) = userdata::starter::create("Cabinet").unwrap();
        let (guest_account, _) = userdata::starter::create(GUEST_NAME).unwrap();
        let machine_id = crate::database::arcade::generate_machine_id();
        crate::database::arcade::insert_machine(&machine_id, "Cabinet", machine_account, guest_account);
        assert!(!orphan_of_a_rebound_card(machine_account));
        assert!(!orphan_of_a_rebound_card(guest_account));

        // An account that no longer exists is not deleted twice
        userdata::delete_account(orphan);
        assert!(!orphan_of_a_rebound_card(orphan));

        crate::database::arcade::delete_machine(&machine_id);
    }

    // bind_card_to is the rule under both entrances - the cabinet's transfer
    // proof and the webui's session - so whatever named the account, a cabinet's
    // own identities are refused, and a provable throwaway goes with its card
    #[test]
    fn bind_card_to_refuses_cabinet_identities_and_takes_the_throwaway_with_the_card() {
        let _lock = crate::runtime::lock_test_data_path();
        use crate::database::arcade as db;

        let (machine_account, _) = userdata::starter::create("Cabinet Bind").unwrap();
        let (guest_account, _) = userdata::starter::create(GUEST_NAME).unwrap();
        let machine_id = db::generate_machine_id();
        db::insert_machine(&machine_id, "Cabinet Bind", machine_account, guest_account);
        let (player, _) = userdata::starter::create("Player").unwrap();

        // The id is validated, not repaired
        assert!(bind_card_to("0123-4567", player).is_err());
        assert!(db::card_user("0123-4567").is_none());

        // Neither cabinet identity may sit behind a card
        let card = "4242424242424242";
        assert!(bind_card_to(card, machine_account).is_err());
        assert!(bind_card_to(card, guest_account).is_err());
        assert!(db::card_user(card).is_none());

        // A throwaway the card was carrying leaves with it...
        let (orphan, _) = userdata::starter::create("2424").unwrap();
        db::set_card(card, orphan);
        assert_eq!(bind_card_to(card, player), Ok(player));
        assert_eq!(db::card_user(card), Some(player));
        assert!(userdata::get_acc_from_uid(orphan)["error"].as_bool().unwrap_or(false), "the throwaway survived");

        // ...but an account somebody played on stays
        let (played, played_token) = userdata::starter::create("2425").unwrap();
        let mut user = userdata::get_acc_from_uid(played);
        user["live_list"].push(object!{ master_live_id: 1100101, level: 4, clear_count: 1, high_score: 1, max_combo: 1 }).unwrap();
        userdata::save_acc(&played_token, user);
        let other = "2525252525252525";
        db::set_card(other, played);
        assert_eq!(bind_card_to(other, player), Ok(player));
        assert_eq!(db::card_user(other), Some(player));
        assert!(!userdata::get_acc_from_uid(played)["error"].as_bool().unwrap_or(false), "a played-on account followed its card");

        db::delete_machine(&machine_id);
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
        db::set_card("4444333322221111", claimed_machine);

        // A player account with a card, belonging to no cabinet at all
        let (player, player_token) = userdata::starter::create("Player").unwrap();
        db::set_card("8888777766665555", player);

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
        assert_eq!(db::card_user("4444333322221111"), Some(claimed_machine));

        // A card-bound player account is never a candidate in the first place
        assert_eq!(userdata::uid_from_login_token(&player_token), player);

        db::delete_machine(&live_id);
    }

    // A card the server has never seen gets an account named after its own last
    // four digits, and the name the player then types at the cabinet lands on
    // that account - and on nothing else. The rename is a second /session for
    // the same card, so every other account has to be immune to it.
    #[test]
    fn a_new_cards_account_takes_the_name_the_player_types() {
        let _lock = crate::runtime::lock_test_data_path();
        use crate::database::arcade as db;

        const TTL: i64 = 30 * 60;
        let now = global::timestamp() as i64;
        let card = "6060606060602345";
        let machine_id = db::generate_machine_id();

        // /api/arcade/session, first call: an unknown card, so the account is
        // created under the card's last four digits and the credit's window opens
        let name = card_account_name(card);
        assert_eq!(name, "2345");
        let (user_id, token) = userdata::starter::create(&name).unwrap();
        db::set_card(card, user_id);
        db::open_card_session(card, &machine_id, now + TTL);

        // ...and the same call again, carrying the name the player typed
        assert!(issued_by_this_credit(card, user_id, now, TTL));
        rename_account(user_id, &token, "Ethan");
        assert_eq!(userdata::get_name_and_rank(user_id)["user_name"].as_str(), Some("Ethan"));

        // Once named it is somebody's account: a second name entry on the same
        // card cannot touch it
        assert!(!issued_by_this_credit(card, user_id, now, TTL), "a named account was still renameable");

        // Neither can one whose credit has ended - the window is the proof
        let later_card = "6060606060601111";
        let (later, _) = userdata::starter::create(&card_account_name(later_card)).unwrap();
        db::set_card(later_card, later);
        db::open_card_session(later_card, &machine_id, now + TTL);
        db::backdate_card_session_for_test(later_card, now - TTL - 1, now + TTL);
        assert!(!issued_by_this_credit(later_card, later, now, TTL), "an old credit could still rename");
        db::backdate_card_session_for_test(later_card, now, now - 1);
        assert!(!issued_by_this_credit(later_card, later, now, TTL), "a closed window could still rename");

        // Nor a card presented at one cabinet renaming through another card's row
        db::open_card_session(later_card, &machine_id, now + TTL);
        assert!(!issued_by_this_credit(card, later, now, TTL), "a different card's id could rename");

        // A phone account behind a card is never renameable, however fresh its
        // window is: it has a transfer password, and its name is its own
        let phone_card = "6060606060602222";
        let (phone, _) = userdata::starter::create(&card_account_name(phone_card)).unwrap();
        userdata::user::migration::save_acc_transfer(phone, "hunter2");
        db::set_card(phone_card, phone);
        db::open_card_session(phone_card, &machine_id, now + TTL);
        assert!(!issued_by_this_credit(phone_card, phone, now, TTL), "a phone account was renameable");

        // And neither is an account that has already played something
        let played_card = "6060606060603333";
        let (played, played_token) = userdata::starter::create(&card_account_name(played_card)).unwrap();
        db::set_card(played_card, played);
        db::open_card_session(played_card, &machine_id, now + TTL);
        assert!(issued_by_this_credit(played_card, played, now, TTL));
        let mut user = userdata::get_acc_from_uid(played);
        user["live_list"].push(object!{ master_live_id: 1100101, level: 4, clear_count: 1, high_score: 1, max_combo: 1 }).unwrap();
        userdata::save_acc(&played_token, user);
        assert!(!issued_by_this_credit(played_card, played, now, TTL), "a played account was renameable");

        // Finally: a cabinet's own identity, which a card may not name at all
        let (machine_account, _) = userdata::starter::create("Cabinet Name").unwrap();
        let (guest_account, _) = userdata::starter::create(GUEST_NAME).unwrap();
        db::insert_machine(&machine_id, "Cabinet Name", machine_account, guest_account);
        let cabinet_card = "6060606060604444";
        db::set_card(cabinet_card, guest_account);
        db::open_card_session(cabinet_card, &machine_id, now + TTL);
        assert!(!issued_by_this_credit(cabinet_card, guest_account, now, TTL), "a cabinet identity was renameable");

        db::delete_machine(&machine_id);
        for id in [user_id, later, phone, played, machine_account, guest_account] {
            userdata::delete_account(id);
        }
    }
}

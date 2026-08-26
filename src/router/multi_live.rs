// Wire format shared with the C# client rewrite; see docs/multi-live-ws-protocol.md.
mod proto;
// Lobby/room state machine, WebSocket-free so it can be tested without a socket.
mod rooms;
// The /multi_live/ws endpoint itself.
mod ws;

use jzon::{object, JsonValue};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{databases, event, event_ranking, global, items, live, userdata, Session, Api};

// The relay's expiry timers (held slots, empty rooms, dead connections). Started once from
// run_server; see ws::start_sweeper for why it is not lazily started by the first upgrade.
pub use ws::start_sweeper;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/multi_live")
            .route("/start", web::post().to(start))
            .route("/end", web::post().to(end))
            // The Photon replacement. Note this sits inside the /api scope, so the
            // handshake goes through webui_fallback and must carry the usual
            // aoharu-asset-version header like every other game request.
            .route("/ws", web::get().to(ws::ws))
    );
}

// Shock.MULTI_LIVE_END_STATUS, the enum RecvMultiLiveEndRData.is_penalty_miss_ratio
// is cast to (MngLiveData.SendMultiLiveEnd).
//   NONE(0)                        - normal result, client processes every reward list.
//   MISS_RATIO_PENALTY_STATUS(1)   - MngLiveData bails out right after user/stamina,
//                                    LiveScene.OnMultiLiveEnd leaves the room and
//                                    returns to the multi-event top. Nothing is awarded.
//   GREAT_PERFECT_LOW_RATIO_STATUS(2) - rewards still apply, the client only raises
//                                    MultiEventLiveResult.IsOpenCautionDialog.
const STATUS_NONE: u8 = 0;
const STATUS_MISS_RATIO_PENALTY: u8 = 1;
const STATUS_GREAT_PERFECT_LOW_RATIO: u8 = 2;

// Shock.COMMON_CONST.RATIO_DIVISOR. Every ratio in masterdata (event_score._eventPointRatio
// / ._eventBoostRatio, live_boost._eventPointRatio, multievent_rankbonus._eventPtBonus,
// multievent_card_bonus._pointBonusRatioList) is stored in 1/10000 and divided by this.
const RATIO_ONE: i64 = 10000;

// Extended-protocol revision this feature requires (X-Protocol-Version, the same ladder
// card.rs=2 / custom_card.rs=3 use). Older client builds carry incompatible multi
// implementations — pre-relay wire framing and pre-rework flows — so every multi_live
// surface (start, end, and the WS upgrade in ws.rs) refuses anything below it.
// 4 = multi-live over the self-hosted WS relay (permanent co-op).
pub const PROTOCOL_VERSION: u32 = 4;

fn protocol_too_old(req: &HttpRequest) -> bool {
    global::client_protocol_version(req) < PROTOCOL_VERSION
}

fn const_value(id: &str, default: i64) -> i64 {
    let raw = &databases::CONST[id]["value"];
    raw.as_str()
        .and_then(|v| v.parse::<i64>().ok())
        .or_else(|| raw.as_i64())
        .unwrap_or(default)
}

pub fn boost_lp(live_boost: i64) -> i64 {
    databases::LIVE_BOOST[live_boost.to_string()]["lp"]
        .as_i64()
        .unwrap_or(10 * live_boost)
}

fn boost_event_point_ratio(live_boost: i64) -> i64 {
    databases::LIVE_BOOST[live_boost.to_string()]["eventPointRatio"]
        .as_i64()
        .unwrap_or(RATIO_ONE * live_boost.max(1))
}

// MusicLevelMst._fullCombo — the note count both client ratio checks divide by.
fn note_count(master_live_id: i64, level: i64) -> Option<i64> {
    let music_id = databases::LIVE_LIST[master_live_id.to_string()]["masterMusicId"].as_i64()?;
    let row = &databases::MUSIC_LEVEL[format!("{}_{}", music_id, level)];
    if row.is_empty() {
        return None;
    }
    let notes = row["fullCombo"].as_i64()?;
    if notes <= 0 { None } else { Some(notes) }
}

// Aoharu.MultiUtil.IsPenalty / IsHalved, evaluated server-side because
// is_penalty_miss_ratio is what the client actually obeys (both MultiUtil helpers
// are unreferenced in the client — they were only ever a local preview).
//
//   IsPenalty: (MULTI_PENALTY_MISS_RATIO / 100) * fullCombo <= miss
//   IsHalved:  perfect + great < (MULTI_EVENT_LIVE_GREAT_PERFECT_NOTES_MIN_RATIO / 100) * fullCombo
//              (skipped for LIVE_LEVEL 1, per `if ((int)level == 1) return false;`)
//
// MultiUtil.IsHalved reads Perfect + Good, but only because the client-side
// MultiLiveResultProtocolData(userId, LiveScore, bool) ctor never assigns Great
// (IL2CPP @4769631) — it has no great count to use. The const is named
// ..._GREAT_PERFECT_NOTES_MIN_RATIO and the server does receive the full LiveScore,
// so perfect + great is used here.
//
// MULTI_PENALTY_NO_PLAY_MISS_RATIO (5000) is unreachable as a real miss ratio and is
// the value the ratio takes when nothing was judged at all: a client that never played
// reports 0 misses, which the 70% rule would wave through.
//
// Both checks mirror MultiUtil's "missing MusicLevelMst row => false" shape.
fn multi_live_end_status(body: &JsonValue) -> u8 {
    let score = &body["live_score"];
    let notes = match note_count(
        body["master_live_id"].as_i64().unwrap_or(0),
        body["level"].as_i64().unwrap_or(0)
    ) {
        Some(n) => n,
        None => return STATUS_NONE
    };

    let perfect = score["perfect"].as_i64().unwrap_or(0);
    let great = score["great"].as_i64().unwrap_or(0);
    let good = score["good"].as_i64().unwrap_or(0);
    let bad = score["bad"].as_i64().unwrap_or(0);
    let miss = score["miss"].as_i64().unwrap_or(0);

    let penalty_ratio = const_value("MULTI_PENALTY_MISS_RATIO", 70);
    let no_play_ratio = const_value("MULTI_PENALTY_NO_PLAY_MISS_RATIO", 5000);

    let judged = perfect + great + good + bad + miss;
    let penalised = if judged == 0 {
        no_play_ratio >= penalty_ratio
    } else {
        miss * 100 >= penalty_ratio * notes
    };
    if penalised {
        return STATUS_MISS_RATIO_PENALTY;
    }

    if body["level"].as_i64().unwrap_or(0) != 1 {
        let min_ratio = const_value("MULTI_EVENT_LIVE_GREAT_PERFECT_NOTES_MIN_RATIO", 50);
        if (perfect + great) * 100 < min_ratio * notes {
            return STATUS_GREAT_PERFECT_LOW_RATIO;
        }
    }

    STATUS_NONE
}

// The event-point yield of one multi live, from event_score.csv (Shock.EventScoreMst).
//
// Neither ew nor the reconstructed client had an existing consumer to copy: EventData
// exposes _eventLivePointBase / _eventPointRatio / _eventBoostRatio as EventLiveBasePoint
// / EventPointRatio / EventBoostRatio (EventData.cs:134-156) but nothing in the client
// ever reads those three properties — the award has always been computed server-side.
// The interpretation used here is the one the field names and the client's own ratio
// convention dictate, and the one this port is specified against:
//
//   points = _eventLivePointBase
//          * _eventPointRatio  / RATIO_DIVISOR      (per-event scaling)
//          * _eventBoostRatio  / RATIO_DIVISOR      (per-event boost worth)
//          * live_boost._eventPointRatio / RATIO_DIVISOR   (the boost actually spent)
//
// The last factor mirrors LiveBoostMst.GetEventPointRatio (LiveBoostMst.cs:131-137),
// which is `_eventPointRatio / RATIO_DIVISOR` — the boost level itself for the stock
// table (boost 3 -> 30000/10000 -> 3).
//
// All four multi events (108/111/115/119) carry base 10, pointRatio 10000, boostRatio
// 35000, so one boost-1 multi live is worth 35 points.
//
// Division is deferred to the end so the x3.5 boost ratio does not truncate to x3 the
// way the client's integer GetEventPointRatio would.
fn event_live_points(event_id: u32, live_boost: i64) -> i64 {
    let row = &databases::EVENT_SCORE[event_id.to_string()];
    if row.is_empty() {
        // Should be unreachable: event_score.csv has a row for every event, including
        // all four multi events. An event with no row is worth nothing rather than
        // silently falling back to an invented constant.
        println!("multi_live: no event_score row for event {event_id}, awarding 0 event points");
        return 0;
    }
    let base = row["eventLivePointBase"].as_i64().unwrap_or(0);
    let point_ratio = row["eventPointRatio"].as_i64().unwrap_or(0);
    let boost_ratio = row["eventBoostRatio"].as_i64().unwrap_or(0);

    base * point_ratio * boost_ratio * boost_event_point_ratio(live_boost)
        / (RATIO_ONE * RATIO_ONE * RATIO_ONE)
}

// The stamina this live actually cost, as recorded by /multi_live/start. This is what
// live_end_ex scales every reward off, and it is deliberately read without reference to
// the backing event: a closed event suppresses scoring, never rewards.
fn recorded_lp(started: Option<&JsonValue>) -> i64 {
    let live_boost = started.and_then(|s| s["live_boost"].as_i64()).unwrap_or(0);
    started
        .and_then(|s| s["use_lp"].as_i64())
        .unwrap_or_else(|| boost_lp(live_boost))
        .max(0)
}

// The event a multi live should actually score against, or None when it should score
// against nothing.
//
// Multi is a permanent feature here, entered from a client-side button rather than from
// a live event, so the client faithfully sends the backing event id (108) even though
// that event is closed and stays closed by choice. Awarding against a closed event would
// write event points and ranking rows for a season that is not running, so the whole
// event-point path stays dormant — and lights up on its own, with no further change
// here, if an event is ever actually opened.
//
// Nothing else about the live is affected: rewards, EXP, bond, missions and the response
// shape all come out of live_end_ex exactly as they do for an in-session event.
fn scoring_event(started: Option<&JsonValue>, now: u64) -> Option<u32> {
    started
        .and_then(|s| s["master_event_id"].as_u32())
        .filter(|id| *id != 0)
        .filter(|id| event::is_in_session(*id, now))
}

// The whole award for one multi live: the event_score yield, the finishing-position
// bonus from multievent_rankbonus, and the GREAT_PERFECT_LOW_RATIO halving.
fn multi_event_points(event_id: u32, live_boost: i64, players: i64, live_rank: i64, status: u8) -> i64 {
    let mut points = event_live_points(event_id, live_boost);
    points = points * (RATIO_ONE + rank_bonus(players, live_rank)) / RATIO_ONE;
    if status == STATUS_GREAT_PERFECT_LOW_RATIO {
        // MultiUtil calls this state "halved" — the caution dialog goes with a
        // reduced yield, not a forfeited one.
        points /= 2;
    }
    points
}

// multievent_rankbonus._eventPtBonus for this party size / finishing position.
// The table only covers 2-4 players with liveRank <= playerCount; anything outside
// it (a solo room, a rank the table has no row for) simply earns no bonus.
fn rank_bonus(player_count: i64, live_rank: i64) -> i64 {
    databases::MULTIEVENT_RANK_BONUS[format!("{}_{}", player_count, live_rank)]["eventPtBonus"]
        .as_i64()
        .unwrap_or(0)
}

// LiveScene.MultiTask4 builds other_live_score_list from MultiPlayManager.AllPlayers,
// so the poster is already one of its entries. Fall back to len + 1 if a caller ever
// sends a list that genuinely excludes itself.
fn player_count(body: &JsonValue, user_id: i64) -> i64 {
    let list = &body["other_live_score_list"];
    let contains_self = list
        .members()
        .any(|s| s["user_id"].as_i64() == Some(user_id));
    let len = list.len() as i64;
    if contains_self { len } else { len + 1 }
}

// Every field of Shock.RecvMultiLiveEndRData, so the penalty path still deserialises
// cleanly. The client reads user/stamina and then returns on status 1, but Notify()
// runs over the whole payload first.
fn barren_response(user: &JsonValue, status: u8) -> JsonValue {
    object!{
        "is_penalty_miss_ratio": status,
        "gem": user["gem"].clone(),
        "clear_master_live_mission_ids": [],
        "user": user["user"].clone(),
        "stamina": user["stamina"].clone(),
        "character_list": [],
        "card_list": [],
        "card_sub_list": [],
        "item_list": [],
        "point_list": [],
        "group_list": [],
        "reward_list": [],
        "gift_list": [],
        "clear_mission_ids": [],
        "event_point_list": user["event_point_list"].clone(),
        "event_point_reward_list": [],
        "ranking_change": [],
        "music_mission_reward_list": [],
        "event_ranking_data": {
            "event_point_rank": 0,
            "next_reward_rank_point": 0,
            "event_score_rank": 0,
            "next_reward_rank_score": 0
        }
    }
}

// Unlike /live/start, a multi live pays its stamina up front: the response reports
// consumed_stamina and the client never sends a boost with /multi_live/end. The
// amount actually taken is stashed on the recorded start payload so /multi_live/end
// can scale rewards off it without charging for it twice.
async fn start(req: HttpRequest, Session { key, mut body }: Session) -> impl Responder {
    // Older clients speak an incompatible multi — refuse before touching any state.
    if protocol_too_old(&req) {
        return Api(None);
    }
    // `token` is the room-party correlation key ("{userId}.{guid}") that all up to four
    // party members post. It is recorded verbatim on the start record and nothing reads
    // anything else out of it. Reconciling the party itself — checking the four results
    // against each other — is still deferred.
    // master_event_id is recorded verbatim and never validated here: multi is a permanent
    // feature entered against a closed event (see scoring_event), so a closed — or absent
    // — event must start a live exactly like an open one. Stamina comes off live_boost
    // alone, so nothing on this path depends on the event being in session.
    let live_boost = body["live_boost"].as_i64().unwrap_or(0);
    let lp = boost_lp(live_boost).max(0);

    let mut user = userdata::get_acc(&key);
    // Settle regen first so the balance clamped against below is current.
    items::lp_modification(&mut user, 0, true);
    let available = user["stamina"]["stamina"].as_u64().unwrap_or(0);
    // Clamped, not refused, and that IS the intended answer.
    //
    // The client gates on stamina at the entry to matching, never at the POST:
    // MultiSelectionView.RoomCreation / OpenRoomSearchDialog and MultiRestart.RestartLive
    // all run StaminaUtils.UseStaminaValue and divert to StaminaChargeScene when it says
    // the balance is short, while MultiEventMatchingScene.ChangeScene — which is what
    // actually calls SendMultiLiveStart, and does so off the host's _MoveScene RPC —
    // computes the cost and drops the "insufficient" flag on the floor. Stamina only ever
    // goes up between the gate and the POST (regen; the boost selector lives on the gated
    // panels), so an honest client cannot arrive here short, and the official server was
    // never asked what to do about it.
    //
    // So this is the unreachable-in-practice branch, and taking what is there is the
    // benign resolution: the live still starts (a party of four must not be broken up by
    // one member's balance), consumed_stamina reports what was really taken, and because
    // every reward scales off that same recorded use_lp — see recorded_lp and live_end_ex —
    // a short live pays out in proportion. Refusing would have to fail a live the other
    // three players are already committed to; charging the full cost would mean inventing
    // negative stamina.
    let consumed = (lp as u64).min(available);
    items::lp_modification(&mut user, consumed, true);
    userdata::save_acc(&key, user);

    body["use_lp"] = consumed.into();
    live::start_live(&key, &body);

    Api(Some(object!{
        "consumed_stamina": consumed
    }))
}

async fn end(req: HttpRequest, Session { key, body }: Session) -> impl Responder {
    // Older clients speak an incompatible multi — refuse before touching any state
    // (in particular before the start record can be consumed).
    if protocol_too_old(&req) {
        return Api(None);
    }
    // Loaded once and reused by every path that answers without playing the live; the
    // live itself re-reads it, because live_end_ex saves the account.
    let account = userdata::get_acc(&key);
    // The user's own clock, so a time-travelling account sees the same event window the
    // rest of the game shows it.
    let uid = account["user"]["id"].as_i64().unwrap_or(0);

    // A body with no live id is answered like a duplicate: it cannot name a start record
    // (matching one on a null id would let it claim a record started with the same
    // malformed body) and nothing downstream can score it.
    if body["master_live_id"].as_i64().is_none() {
        println!("multi_live/end: uid {} posted no master_live_id — nothing to end", uid);
        return Api(Some(barren_response(&account, STATUS_NONE)));
    }

    // The started-live record is what makes an end legitimate, and it is CLAIMED here:
    // take_started_live returns it and removes it in one transaction, before anything is
    // awarded, so of N ends arriving together exactly one can proceed. The consumption
    // used to be a side effect of get_end_live_deck_id deep inside live_end_ex — which
    // only fired when the record carried a numeric deck_slot, and fired long after the
    // award had been decided, so a re-POST could be paid twice over.
    //
    // Failing the take means this live was already ended: two clients signed in to the
    // SAME account both POST /multi_live/end for the one shared record, or the client
    // retried a request whose response it never saw. Granting again would double-count the
    // clear, the clear-rate counter, the play-count mission and the flat 17001001 drop.
    // So the duplicate is answered from current state: a well-formed result the client can
    // display, awarding nothing.
    let started = live::take_started_live(&key, &body);
    let started = started.as_ref();
    if started.is_none() {
        println!("multi_live/end: uid {} has no start record to spend — answering barren", uid);
        return Api(Some(barren_response(&account, STATUS_NONE)));
    }

    let live_boost = started.and_then(|s| s["live_boost"].as_i64()).unwrap_or(0);
    let lp_used = recorded_lp(started);
    let event_id = scoring_event(started, global::set_time(global::timestamp(), uid, false));

    let status = multi_live_end_status(&body);
    println!(
        "multi_live/end: uid {} score {} miss-ratio status {} ({})",
        uid,
        body["live_score"]["score"].as_i64().unwrap_or(-1),
        status,
        match status {
            STATUS_MISS_RATIO_PENALTY => "PENALTY — results voided",
            2 => "great/perfect low — points halved",
            _ => "ok",
        }
    );

    if status == STATUS_MISS_RATIO_PENALTY {
        // The client discards the result and leaves the room, so nothing is granted. The
        // record is already gone (claimed above), so the live is not left hanging until it
        // expires either — and a re-POST of the same penalised result lands on the
        // no-record branch rather than back here.
        return Api(Some(barren_response(&account, status)));
    }

    // /multi_live/end carries neither live_boost nor deck_slot; use_lp is what
    // live_end scales exp / gold / bond / mission rewards off.
    let mut end_body = body.clone();
    end_body["use_lp"] = lp_used.into();
    if end_body["deck_slot"].is_null() {
        if let Some(slot) = started.and_then(|s| s["deck_slot"].as_i32()) {
            end_body["deck_slot"] = slot.into();
        }
    }

    // A multi live scores like the official server did: no high score, no score board —
    // the client's own result screen says so. Private (join-by-code) parties are no
    // exception (Ethan 2026-08-12; an earlier build recorded private-party scores, and
    // the room-privacy plumbing that told the two apart left with that behaviour). The
    // clear count and max combo still record either way — the live really was played.
    let mut rv = live::live_end_ex(&req, &key, &end_body, false, false, false);

    rv["is_penalty_miss_ratio"] = status.into();
    // Fields RecvMultiLiveEndRData declares that live_end does not emit.
    rv["card_list"] = jzon::array![];
    rv["card_sub_list"] = jzon::array![];
    rv["group_list"] = jzon::array![];
    rv["music_mission_reward_list"] = jzon::array![];
    // MngLiveData.SendMultiLiveEnd dereferences event_ranking_data unconditionally,
    // so it must be an object even when this live belongs to no event.
    rv["event_ranking_data"] = object!{
        "event_point_rank": 0,
        "next_reward_rank_point": 0,
        "event_score_rank": 0,
        "next_reward_rank_score": 0
    };

    if let Some(event_id) = event_id {
        // live_end already saved the account; re-read it before touching event points.
        let mut user = userdata::get_acc(&key);
        let user_id = user["user"]["id"].as_i64().unwrap_or(0);

        let players = player_count(&body, user_id);
        let live_rank = body["multi_live_rank"].as_i64().unwrap_or(0);

        let points = multi_event_points(event_id, live_boost, players, live_rank, status);

        event::give_event_points(event_id, points, &mut user);
        userdata::save_acc(&key, user.clone());

        let total = event::get_points(event_id, &user);
        event_ranking::live_completed(event_id, user_id, total, 0);
        let rank = event::get_rank(event_id, user_id as u64);

        let mut event = event::get_event_data(&key, event_id);
        event["point_ranking"]["point"] = total.into();
        event["point_ranking"]["rank"] = rank.into();
        event::save_event_data(&key, event_id, event);

        rv["event_point_list"] = user["event_point_list"].clone();
        rv["event_ranking_data"] = object!{
            "event_point_rank": rank,
            "next_reward_rank_point": 0,
            "event_score_rank": rank,
            "next_reward_rank_score": 0
        };
    }

    Api(Some(rv))
}

#[cfg(test)]
mod tests {
    use super::*;
    use super::proto::{ClientMsg, Map, Value};
    use jzon::array;
    use std::time::Instant;

    const STOCK_LIVE_ID: i64 = 1100101;

    #[test]
    fn multi_consts_resolve_from_masterdata() {
        assert_eq!(const_value("MULTI_PENALTY_MISS_RATIO", -1), 70);
        assert_eq!(const_value("MULTI_PENALTY_NO_PLAY_MISS_RATIO", -1), 5000);
        assert_eq!(const_value("MULTI_EVENT_LIVE_GREAT_PERFECT_NOTES_MIN_RATIO", -1), 50);
        assert_eq!(const_value("NOT_A_CONST", -1), -1);
    }

    #[test]
    fn boost_costs_come_from_live_boost() {
        assert_eq!(boost_lp(1), 10);
        assert_eq!(boost_lp(10), 100);
        assert_eq!(boost_event_point_ratio(1), RATIO_ONE);
        assert_eq!(boost_event_point_ratio(3), 3 * RATIO_ONE);
    }

    // The four multi events from multievent_setting.csv, all sharing one event_score row
    // shape: _eventLivePointBase 10, _eventPointRatio 10000, _eventBoostRatio 35000.
    const MULTI_EVENTS: [u32; 4] = [108, 111, 115, 119];

    #[test]
    fn event_score_rows_back_every_multi_event() {
        for event_id in MULTI_EVENTS {
            let row = &databases::EVENT_SCORE[event_id.to_string()];
            assert!(!row.is_empty(), "event_score row missing for event {event_id}");
            assert_eq!(row["eventLivePointBase"].as_i64(), Some(10));
            assert_eq!(row["eventPointRatio"].as_i64(), Some(RATIO_ONE));
            assert_eq!(row["eventBoostRatio"].as_i64(), Some(35000));
        }
    }

    #[test]
    fn event_live_points_derive_from_event_score() {
        // 10 * (10000/10000) * (35000/10000) * boost — and the x3.5 must not truncate
        // to x3 on the way through.
        assert_eq!(event_live_points(108, 1), 35);
        assert_eq!(event_live_points(108, 2), 70);
        assert_eq!(event_live_points(108, 10), 350);
        for event_id in MULTI_EVENTS {
            assert_eq!(event_live_points(event_id, 1), 35);
        }
    }

    #[test]
    fn event_live_points_fall_back_to_zero_without_a_row() {
        assert!(databases::EVENT_SCORE["99999"].is_empty());
        assert_eq!(event_live_points(99999, 1), 0);
        assert_eq!(multi_event_points(99999, 1, 4, 1, STATUS_NONE), 0);
    }

    #[test]
    fn multi_event_points_layer_rank_bonus_and_halving() {
        // 1st of 4 is +30% (multievent_rankbonus 4/1 = 3000).
        assert_eq!(multi_event_points(108, 1, 4, 1, STATUS_NONE), 45);
        // Last place earns the flat yield.
        assert_eq!(multi_event_points(108, 1, 4, 4, STATUS_NONE), 35);
        // The caution state halves whatever the bonus produced.
        assert_eq!(multi_event_points(108, 1, 4, 1, STATUS_GREAT_PERFECT_LOW_RATIO), 22);
        assert_eq!(multi_event_points(108, 1, 4, 4, STATUS_GREAT_PERFECT_LOW_RATIO), 17);
    }

    // release_label 223061504, event 108's window: 2023/06/19 05:00:00 - 2023/06/28 04:59:59.
    fn during_multi_event_108() -> u64 {
        global::parse_datetime("2023/06/20 12:00:00").unwrap()
    }

    #[test]
    fn masterdata_datetimes_round_trip() {
        let t = global::parse_datetime("2023/06/19 5:00:00").unwrap();
        assert_eq!(global::format_datetime(t), "2023-06-19 05:00:00");
        // A bare date is midnight, and the blank cells the evergreen label uses parse
        // to nothing rather than to the epoch.
        assert_eq!(
            global::parse_datetime("2023/06/19"),
            global::parse_datetime("2023/06/19 0:00:00")
        );
        assert_eq!(global::parse_datetime(""), None);
        assert_eq!(global::parse_datetime("null"), None);
    }

    #[test]
    fn the_multi_backing_event_is_closed_by_design() {
        // The evergreen label has no window at all and is always open.
        assert!(event::release_label_is_open(1, during_multi_event_108()));

        // Event 108 is in session only inside its own long-past label window...
        assert!(event::is_in_session(108, during_multi_event_108()));
        // ...and is closed now, which is the permanent-feature state multi runs in.
        // global::timestamp() rather than set_time: set_time reads crate::get_args(),
        // whose clap parser chokes on the test-harness filter argument.
        let now = global::timestamp();
        assert!(!event::is_in_session(108, now), "event 108 must stay closed");
        for event_id in MULTI_EVENTS {
            assert!(!event::is_in_session(event_id, now));
        }
    }

    #[test]
    fn a_closed_backing_event_scores_against_nothing() {
        let started = object!{ master_event_id: 108, live_boost: 1 };

        // Closed today: the whole event-point path is skipped.
        // global::timestamp() rather than set_time: set_time reads crate::get_args(),
        // whose clap parser chokes on the test-harness filter argument.
        let now = global::timestamp();
        assert_eq!(scoring_event(Some(&started), now), None);

        // But the gate is a window test, not a hardcoded off switch — open the event
        // and scoring resumes on its own.
        assert_eq!(scoring_event(Some(&started), during_multi_event_108()), Some(108));

        // A live with no backing event at all, and a missing start record, score nothing.
        assert_eq!(scoring_event(Some(&object!{ master_event_id: 0 }), during_multi_event_108()), None);
        assert_eq!(scoring_event(None, during_multi_event_108()), None);
    }

    // The end-to-end path cannot be exercised here: live_end_ex calls items::get_region,
    // which reaches crate::get_args(), whose clap parser rejects the test harness's own
    // filter argument. (live.rs's tests avoid live_end for the same reason.) What is
    // testable, and what actually matters, is that the reward input is derived with no
    // reference to the event window — so a closed event can only ever suppress scoring.
    #[test]
    fn a_closed_event_still_pays_its_normal_rewards() {
        let started = object!{ master_event_id: 108, live_boost: 1, use_lp: 10, deck_slot: 2 };
        let open = during_multi_event_108();
        // global::timestamp() rather than set_time: set_time reads crate::get_args(),
        // whose clap parser chokes on the test-harness filter argument.
        let closed = global::timestamp();

        // The event gate is the only thing that moves between the two.
        assert_eq!(scoring_event(Some(&started), open), Some(108));
        assert_eq!(scoring_event(Some(&started), closed), None);

        // The reward input is identical either way, and identical to what an eventless
        // live would get for the same boost.
        assert_eq!(recorded_lp(Some(&started)), 10);
        assert_eq!(recorded_lp(Some(&object!{ live_boost: 1, use_lp: 10 })), 10);
        // A start record with no recorded charge still falls back to the boost's cost.
        assert_eq!(recorded_lp(Some(&object!{ master_event_id: 108, live_boost: 3 })), 30);
        assert_eq!(recorded_lp(None), 0);

        // And a suppressed event awards nothing, where an open one would pay 35.
        assert_eq!(multi_event_points(108, 1, 4, 4, STATUS_NONE), 35);
    }

    // --- duplicate protection (the claim that gates the award) ----------------------
    //
    // /multi_live/end awards if and only if take_started_live hands it the record, and the
    // record can be handed out exactly once. These drive that claim directly: the handler
    // itself cannot be called from a test (live_end_ex reaches items::get_region, which
    // reaches crate::get_args(), whose clap parser rejects the harness's own arguments).

    // Two clients signed in to one account share a single started-live record, so the
    // second /multi_live/end arrives after the first consumed it. The handler answers that
    // from current state and grants nothing; this pins the state machine it keys off.
    #[test]
    fn a_second_end_for_one_session_finds_no_record_to_spend() {
        let _lock = crate::runtime::lock_test_data_path();
        let token = "multi_duplicate_end";

        let start_body = object!{
            master_live_id: STOCK_LIVE_ID,
            level: 4,
            deck_slot: 1,
            live_boost: 1,
            master_event_id: 108,
            use_lp: 10
        };
        live::start_live(token, &start_body);

        // First end: the record is there and pays for the live, and claiming it is what
        // the handler does BEFORE it awards anything.
        let started = live::take_started_live(token, &start_body);
        assert!(started.is_some());
        assert_eq!(recorded_lp(started.as_ref()), 10);

        // Second end: nothing left to spend, which is the duplicate signal the handler
        // short-circuits on, and the reward scale is zero even if it did not.
        let again = live::take_started_live(token, &start_body);
        assert!(again.is_none(), "the record must not survive the first end");
        assert_eq!(recorded_lp(again.as_ref()), 0);
        assert_eq!(scoring_event(again.as_ref(), during_multi_event_108()), None);
        // And it stays gone however many times the client re-POSTs.
        assert!(live::take_started_live(token, &start_body).is_none());
        assert!(live::get_started_live(token, &start_body).is_none());
    }

    // The consumption used to hide inside get_end_live_deck_id, behind
    // `record["deck_slot"].as_i32()?` — a start whose body carried no numeric deck_slot
    // left the record in place, so every re-POST awarded again. Claiming is now about the
    // record's existence and nothing else.
    #[test]
    fn a_start_with_no_deck_slot_is_still_consumed_by_the_first_end() {
        let _lock = crate::runtime::lock_test_data_path();
        let token = "multi_no_deck_slot";

        for start_body in [
            // No deck_slot at all...
            object!{ master_live_id: STOCK_LIVE_ID, level: 4, live_boost: 1, use_lp: 10 },
            // ...and one that is present but not a number.
            object!{ master_live_id: STOCK_LIVE_ID, level: 4, live_boost: 1, use_lp: 10, deck_slot: "1" }
        ] {
            live::start_live(token, &start_body);
            let first = live::take_started_live(token, &start_body);
            assert!(first.is_some(), "the record must be claimable without a deck_slot");
            assert_eq!(recorded_lp(first.as_ref()), 10);

            let second = live::take_started_live(token, &start_body);
            assert!(second.is_none(), "a re-POST must find nothing left to award off");
        }
    }

    // Two ends landing together — the normal case for one account signed in twice, and
    // the one the old shape got wrong: both read the record, both passed the guard, both
    // awarded. The claim is a single transaction, so exactly one of any number of racing
    // ends can proceed.
    #[test]
    fn concurrent_ends_claim_the_record_exactly_once() {
        let _lock = crate::runtime::lock_test_data_path();
        let token = "multi_concurrent_end";

        let start_body = object!{
            master_live_id: STOCK_LIVE_ID,
            level: 4,
            deck_slot: 1,
            live_boost: 1,
            master_event_id: 108,
            use_lp: 10
        };
        // Create the account before the threads start: the claim itself is atomic, the
        // lazy account creation behind it is not, and that is not what is under test.
        live::start_live(token, &start_body);

        let claims = std::sync::atomic::AtomicUsize::new(0);
        std::thread::scope(|s| {
            for _ in 0..8 {
                s.spawn(|| {
                    if live::take_started_live(token, &start_body).is_some() {
                        claims.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    }
                });
            }
        });

        assert_eq!(
            claims.load(std::sync::atomic::Ordering::SeqCst),
            1,
            "exactly one of eight simultaneous ends may award"
        );
        assert!(live::get_started_live(token, &start_body).is_none());
    }

    #[test]
    fn rank_bonus_matches_multievent_rankbonus() {
        assert_eq!(rank_bonus(4, 1), 3000);
        assert_eq!(rank_bonus(4, 4), 0);
        assert_eq!(rank_bonus(2, 1), 1000);
        // Rows the table does not cover earn nothing rather than panicking.
        assert_eq!(rank_bonus(1, 1), 0);
        assert_eq!(rank_bonus(4, 9), 0);
    }

    fn score(perfect: i64, great: i64, good: i64, bad: i64, miss: i64) -> JsonValue {
        object!{
            master_live_id: STOCK_LIVE_ID,
            level: 4,
            live_score: {
                perfect: perfect, great: great, good: good, bad: bad, miss: miss
            }
        }
    }

    #[test]
    fn end_status_follows_multiutil_ratios() {
        let notes = note_count(STOCK_LIVE_ID, 4).expect("music_level row");

        // A clean full combo is normal.
        assert_eq!(multi_live_end_status(&score(notes, 0, 0, 0, 0)), STATUS_NONE);

        // 70% or more of the notes missed trips the penalty.
        assert_eq!(multi_live_end_status(&score(0, 0, 0, 0, notes)), STATUS_MISS_RATIO_PENALTY);

        // Nothing judged at all is the "no play" case the 70% rule would wave through.
        assert_eq!(multi_live_end_status(&score(0, 0, 0, 0, 0)), STATUS_MISS_RATIO_PENALTY);

        // Under half the notes as perfect/great, but few enough misses to avoid the
        // penalty, is the caution ("halved") state.
        let goods = notes - (notes / 4) - 1;
        assert_eq!(
            multi_live_end_status(&score(notes / 4, 0, goods, 0, 1)),
            STATUS_GREAT_PERFECT_LOW_RATIO
        );

        // LIVE_LEVEL 1 is exempt from the great/perfect check.
        let mut beginner = score(0, 0, 0, 0, 0);
        beginner["level"] = 1.into();
        beginner["live_score"]["good"] = note_count(STOCK_LIVE_ID, 1).unwrap().into();
        assert_eq!(multi_live_end_status(&beginner), STATUS_NONE);

        // A live with no MusicLevelMst row never penalises, like MultiUtil.
        let mut unknown = score(0, 0, 0, 0, 0);
        unknown["master_live_id"] = 999999999i64.into();
        assert_eq!(multi_live_end_status(&unknown), STATUS_NONE);
    }

    #[test]
    fn player_count_does_not_double_count_the_poster() {
        let body = object!{
            other_live_score_list: array![
                object!{ user_id: 7 },
                object!{ user_id: 8 },
                object!{ user_id: 9 }
            ]
        };
        // LiveScene includes the poster in the list.
        assert_eq!(player_count(&body, 8), 3);
        // A list that omits the poster still yields the real party size.
        assert_eq!(player_count(&body, 42), 4);
    }
}

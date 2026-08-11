use jzon::{JsonValue, object, array};
use actix_web::{web, HttpRequest, Responder};
use rand::RngExt;

use crate::include_file;
use crate::router::{userdata, global, databases, Body, Session, Api};

// I believe(?) this is all?
const STAR_EVENT_IDS: [u32; 3] = [127, 135, 139];

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/event")
            .route("", web::post().to(event))
            .route("/star_event", web::post().to(star_event))
            .route("/set/member", web::post().to(set_member))
            .route("/ranking", web::post().to(ranking))
    );
    cfg.service(
        web::scope("/event_star_live")
            .route("/start", web::post().to(crate::router::live::event_start))
            .route("/change_target_music", web::post().to(change_target_music))
            .route("/end", web::post().to(event_end))
            .route("/skip", web::post().to(event_skip))
    );
}

// Whether a release_label window is open at `now`. A blank _openedAt has always been
// open and a blank _closedAt never closes, which is how the evergreen label (id 1) is
// expressed. _releaseStatus other than 1 is never released at all.
pub fn release_label_is_open(label_id: i64, now: u64) -> bool {
    let label = &databases::RELEASE_LABEL[label_id.to_string()];
    if label.is_empty() || label["releaseStatus"].as_i64().unwrap_or(0) != 1 {
        return false;
    }
    if let Some(opened) = global::parse_datetime(&label["openedAt"].to_string()) {
        if now < opened {
            return false;
        }
    }
    if let Some(closed) = global::parse_datetime(&label["closedAt"].to_string()) {
        if now > closed {
            return false;
        }
    }
    true
}

// Whether an event is currently running, by its own release label window. ew had no
// in-session test before this — nothing else evaluates release_label — so this is the
// one place to extend if the event listing ever needs the same question answered.
pub fn is_in_session(event_id: u32, now: u64) -> bool {
    let event = &databases::EVENTS[event_id.to_string()];
    if event.is_empty() {
        return false;
    }
    release_label_is_open(event["masterReleaseLabelId"].as_i64().unwrap_or(0), now)
}

pub fn get_event_data(key: &str, event_id: u32) -> JsonValue {
    let mut event = userdata::get_acc_event(key);
    let is_star_event = STAR_EVENT_IDS.contains(&event_id);
    //println!("is_star_event: {}, {}", is_star_event, event_id);

    // Broken event data.. Should no longer be possible.
    if is_star_event && event[event_id.to_string()]["star_event"]["star_music_list"].len() > 5 {
        event.remove(&event_id.to_string());
    }

    if event[event_id.to_string()].is_empty() {
        event[event_id.to_string()] = jzon::parse(&include_file!("src/router/userdata/new_user_event.json")).unwrap();
        if is_star_event {
            let mut ev = event[event_id.to_string()].clone();
            init_star_event(&mut ev);
            save_event_data(key, event_id, ev);
            event = userdata::get_acc_event(key);
        }
    }

    if is_star_event && event["star_last_reset"][event_id.to_string()].as_u64().unwrap_or(0) <= global::timestamp_since_midnight() {
        event["star_last_reset"][event_id.to_string()] = (global::timestamp_since_midnight() + (24 * 60 * 60)).into();
        event[event_id.to_string()]["star_event"]["star_event_bonus_daily_count"] = 0.into();
    }

    normalise_event_shape(&mut event[event_id.to_string()]);

    event[event_id.to_string()].clone()
}

// The client's /api/event response types are [Serializable] C# classes, and Unity's
// JsonUtility maps them structurally: `score_ranking`, `member_ranking` and `lottery_box`
// are OBJECTS (Shock.ProtocolData.ScoreRanking / MemberRanking / LotteryBox), not arrays.
// new_user_event.json used to seed them as `[]`, which the client cannot bind - and
// MngEventData.SendGetEvent's success callback dereferences `r.data.score_ranking` and
// friends unguarded, so a mis-shaped payload takes out the whole recv and its `onComplete`
// is never called. That callback is the completion of a GATING TaskFlow.Step in
// LiveRestartSelectScene.ReloadScene, so the scene hangs on its loading screen forever.
//
// The template is fixed, but accounts created before that keep their stored blob, so the
// shapes are normalised on the way out as well. Coercion only ever replaces a value the
// client could not have parsed anyway; a well-formed object is left exactly as it is.
// `set_member` already writes member_ranking as an object, which is the shape this agrees
// with.
fn normalise_event_shape(event: &mut JsonValue) {
    fn ensure_object(slot: &mut JsonValue, template: JsonValue) {
        if !slot.is_object() {
            *slot = template;
        } else {
            for (key, value) in template.entries() {
                if slot[key].is_null() {
                    slot[key] = value.clone();
                }
            }
        }
    }

    ensure_object(&mut event["point_ranking"], object! { rank: 0, point: 0 });
    ensure_object(&mut event["score_ranking"], object! { all_rank: 0, group_rank: 0, score: 0 });
    ensure_object(
        &mut event["member_ranking"],
        object! { master_character_id: 0, rank: 0, point: 0 },
    );
    ensure_object(
        &mut event["lottery_box"],
        object! { master_lottery_id: 0, reset_count: 0, draw_count_list: array![] },
    );
    // Read by EventData.SetMultiParameter (help count, penalty window, disconnect flag);
    // absent keys bind as 0/false, but sending them keeps the payload self-describing.
    for key in ["is_disconnected", "help_count", "penalty_remaining_time"] {
        if event[key].is_null() {
            event[key] = 0.into();
        }
    }
    if !event["mission_list"].is_array() {
        event["mission_list"] = array![];
    }
}

pub fn save_event_data(key: &str, event_id: u32, data: JsonValue) {
    let mut event = userdata::get_acc_event(key);

    // Check for old version of event data
    if !event["event_data"].is_empty() {
        event = object!{};
    }

    event[event_id.to_string()] = data;

    userdata::save_acc_event(key, event);
}

fn get_random_song() -> JsonValue {
    let mut rng = rand::rng();
    let random_number = rng.random_range(0..=databases::LIVES.len());
    object!{
        song: databases::LIVES[random_number]["masterMusicId"].clone(),
        score: (databases::LIVES[random_number]["scoreC"].as_f64().unwrap() * 1.75).round() as i64
    }
}

fn switch_music(event: &mut JsonValue, index: i32) {
    if !(1..=5).contains(&index) {
        return;
    }

    let mut i: i32 = -1;
    for (j, live) in event["star_event"]["star_music_list"].members().enumerate() {
        if live["position"] == index {
            i = j as i32;
            break;
        }
    }
    if i >= 0 {
        event["star_event"]["star_music_list"].array_remove(i as usize);
    }

    let random_song = get_random_song();
    let to_push = object!{
        master_music_id: random_song["song"].clone(),
        position: index,
        is_cleared: 0,
        goal_score: random_song["score"].clone()
    };
    event["star_event"]["star_music_list"].push(to_push).unwrap();
}

fn init_star_event(event: &mut JsonValue) {
    if event["star_event"]["star_level"].as_i32().unwrap() != 0 {
        return;
    }
    event["star_event"]["star_level"] = 1.into();
    switch_music(event, 1);
    switch_music(event, 2);
    switch_music(event, 3);
    switch_music(event, 4);
    switch_music(event, 5);
}

async fn event(Session { key, body }: Session) -> impl Responder {
    let master_event_id = body["master_event_id"].as_u32().unwrap();
    let mut event = get_event_data(&key, master_event_id);

    let is_star_event = STAR_EVENT_IDS.contains(&master_event_id);

    if is_star_event {
        let user = userdata::get_acc(&key);
        let old = event["star_event"]["star_level"].as_i64().unwrap();
        event["star_event"]["star_level"] = get_star_rank(get_points(master_event_id, &user)).into();
        let leveled = old != event["star_event"]["star_level"].as_i64().unwrap();

        let mut all_clear = 1;
        for data in event["star_event"]["star_music_list"].members() {
            if data["is_cleared"] == 0 {
                all_clear = 0;
            }
        }
        if all_clear == 1 {
            event["star_event"]["star_music_list"] = array![];
            switch_music(&mut event, 1);
            switch_music(&mut event, 2);
            switch_music(&mut event, 3);
            switch_music(&mut event, 4);
            switch_music(&mut event, 5);
            save_event_data(&key, master_event_id, event.clone());
        }


        event["point_ranking"]["point"] = get_points(master_event_id, &user).into();
        event["point_ranking"]["rank"] = get_rank(master_event_id, user["user"]["id"].as_u64().unwrap()).into();

        if leveled {
            save_event_data(&key, master_event_id, event.clone());
            event["star_event"]["is_star_event_update"] = 1.into();
        } else {
            save_event_data(&key, master_event_id, event.clone());
        }
    }

    Api(Some(event))
}

async fn star_event(Session { key, body }: Session) -> impl Responder {
    let user = userdata::get_acc(&key);
    let master_event_id = body["master_event_id"].as_u32().unwrap();

    let mut event = get_event_data(&key, master_event_id);

    let mut star_event = event["star_event"].clone();
    star_event["is_inherited_level_reward"] = 0.into();

    event["star_event"]["star_level"] = get_star_rank(get_points(master_event_id, &user)).into();
    star_event["is_star_level_up"] = 1.into();

    save_event_data(&key, master_event_id, event.clone());

    Api(Some(object!{
        star_event: star_event,
        gift_list: [],
        reward_list: []
    }))
}

async fn change_target_music(Session { key, body }: Session) -> impl Responder {
    let master_event_id = body["master_event_id"].as_u32().unwrap();

    let mut event = get_event_data(&key, master_event_id);

    event["star_event"]["music_change_count"] = (event["star_event"]["music_change_count"].as_i32().unwrap() + 1).into();

    switch_music(&mut event, body["position"].as_i32().unwrap());

    save_event_data(&key, master_event_id, event.clone());

    Api(Some(event["star_event"].clone()))
}

async fn set_member(Session { key, body }: Session) -> impl Responder {
    let master_event_id = body["master_event_id"].as_u32().unwrap();

    let mut event = get_event_data(&key, master_event_id);

    event["member_ranking"] = object!{
        master_character_id: body["master_character_id"].clone(),
        rank: 0,
        point: 0
    };

    save_event_data(&key, master_event_id, event.clone());

    Api(Some(object!{
        event_member: event["member_ranking"].clone()
    }))
}

pub fn get_rank(event: u32, user_id: u64) -> u32 {
    let scores = crate::router::event_ranking::get_raw_info(event);

    let mut i=1;
    for score in scores.members() {
        if score["user"] == user_id {
            return i;
        }
        i += 1;
    }
    0
}

async fn ranking(req: HttpRequest, Body(body): Body) -> impl Responder {
    let protocol = crate::router::global::client_protocol_version(&req);
    let master_event_id = body["master_event_id"].as_u32().unwrap();
    let scores = crate::router::event_ranking::get_scores_json().await[master_event_id as usize].clone();
    let mut rv = array![];
    let mut i=1;
    let start = if body["user_id"] == 0 { body["start_rank"].as_u32().unwrap() } else { get_rank(master_event_id, body["user_id"].as_u64().unwrap()) };
    for score in scores.members() {
        if i >= start && start + body["count"].as_u32().unwrap() >= i {
            let mut entry = score.clone();
            crate::router::tools::guest::proxy_user_cards(&mut entry["user_detail"], protocol);
            rv.push(entry).unwrap();
            i += 1;
        }
        if start + body["count"].as_u32().unwrap() >= i {
            break;
        }
    }

    Api(Some(object!{
        ranking_detail_list: rv
    }))
}

const POINTS_PER_LEVEL: i64 = 65;

fn get_star_rank(points: i64) -> i64 {
    ((points - (points % POINTS_PER_LEVEL)) / POINTS_PER_LEVEL) + 1
}

const LIMIT_COINS: i64 = 2000000000;

// The row is keyed by BOTH the event and the point type, exactly as get_points reads it
// back. Matching on the type alone credited whichever event's row happened to come first
// in the list — an account that had ever played a different event got its points there,
// and get_points (which does check the id) then reported 0 for the event just played.
pub fn give_event_points(event_id: u32, amount: i64, user: &mut JsonValue) -> bool {
    let mut has = false;
    for data in user["event_point_list"].members_mut() {
        if data["type"] == 1 && data["master_event_id"] == event_id {
            has = true;
            let new_amount = data["amount"].as_i64().unwrap() + amount;
            if new_amount > LIMIT_COINS {
                return true;
            }
            data["amount"] = new_amount.into();
            break;
        }
    }
    if !has {
        user["event_point_list"].push(object!{
            master_event_id: event_id,
            type: 1,
            amount: amount,
            reward_status: []
        }).unwrap();
    }
    false
}

pub fn get_points(event_id: u32, user: &JsonValue) -> i64 {
    for data in user["event_point_list"].members() {
        if data["type"] == 1 && data["master_event_id"] == event_id {
            return data["amount"].as_i64().unwrap()
        }
    }
    0
}

fn event_live(req: &HttpRequest, key: &str, body: &JsonValue, skipped: bool) -> Option<JsonValue> {
    let event_id = if skipped {
        body["master_event_id"].as_u32().unwrap()
    } else {
        crate::router::live::get_end_live_event_id(key, body)?
    };

    let mut resp = crate::router::live::live_end(req, key, body, skipped);
    let mut event = get_event_data(&key, event_id);
    let mut user = userdata::get_acc(&key);

    let live_id = databases::LIVE_LIST[body["master_live_id"].to_string()]["masterMusicId"].as_i64().unwrap();
    let raw_score = body["live_score"]["score"].as_u64().or_else(|| resp["high_score"].as_u64()).unwrap_or(0);

    let bonus_event = event["star_event"]["star_event_bonus_daily_count"].as_u64().unwrap();
    let bonus_play_times = event["star_event"]["star_event_play_times_bonus_count"].as_u64().unwrap();
    let score = raw_score + (raw_score * bonus_event) + (raw_score * bonus_play_times);

    let mut all_clear = 1;
    let mut cleared = false;
    for data in event["star_event"]["star_music_list"].members_mut() {
        if data["master_music_id"] == live_id && score >= data["goal_score"].as_u64().unwrap() {
            data["is_cleared"] = 1.into();
            cleared = true;
        }
        if data["is_cleared"] == 0 {
            all_clear = 0;
        }
    }

    if cleared {
        event["star_event"]["star_event_bonus_daily_count"] = (event["star_event"]["star_event_bonus_daily_count"].as_u32().unwrap() + 1).into();
        event["star_event"]["star_event_bonus_count"] = (event["star_event"]["star_event_bonus_count"].as_u32().unwrap() + 1).into();
        event["star_event"]["star_event_play_times_bonus_count"] = (event["star_event"]["star_event_play_times_bonus_count"].as_u32().unwrap() + 1).into();

        give_event_points(event_id, 31, &mut user);
        userdata::save_acc(&key, user.clone());
    }

    crate::router::event_ranking::live_completed(event_id, user["user"]["id"].as_i64().unwrap(), get_points(event_id, &user), event["star_event"]["star_level"].as_i64().unwrap());

    resp["star_event_bonus_list"] = object!{
        "star_event_bonus": bonus_event,
        "star_event_bonus_score": bonus_event * raw_score,
        "star_play_times_bonus": bonus_play_times,
        "star_play_times_bonus_score": bonus_play_times * raw_score,
        "card_bonus": 0,
        "card_bonus_score": 0
    };


    resp["event_point_list"] = user["event_point_list"].clone();
    resp["event_ranking_data"] = object! {
        "event_point_rank": event["point_ranking"]["point"].clone(),
        "next_reward_rank_point": 0,
        "event_score_rank": get_rank(event_id, user["user"]["id"].as_u64().unwrap()),
        "next_reward_rank_score": 0,
        "next_reward_rank_level": 0
    };

    resp["is_star_all_clear"] = all_clear.into();
    resp["star_level"] = event["star_event"]["star_level"].clone();
    resp["music_data"] = event["star_event"]["star_music_list"].clone();
    resp["total_score"] = score.into();
    resp["star_event"] = event["star_event"].clone();

    save_event_data(&key, event_id, event);

    //println!("{}", resp);
    Some(resp)
}

async fn event_end(req: HttpRequest, Session { key, body }: Session) -> impl Responder {
    Api(event_live(&req, &key, &body, false))
}

async fn event_skip(req: HttpRequest, Session { key, body }: Session) -> impl Responder {
    Api(event_live(&req, &key, &body, true))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The client binds these with Unity's JsonUtility against [Serializable] classes:
    // ScoreRanking { all_rank, group_rank, score }, MemberRanking { master_character_id,
    // rank, point }, PointRanking { rank, point }, LotteryBox { master_lottery_id,
    // reset_count, draw_count_list }. An array where an object is expected cannot bind, and
    // MngEventData.SendGetEvent's callback dereferences them unguarded.
    #[test]
    fn the_new_user_template_already_has_the_client_shapes() {
        let template: JsonValue =
            jzon::parse(&include_file!("src/router/userdata/new_user_event.json")).unwrap();
        for key in ["point_ranking", "score_ranking", "member_ranking", "lottery_box"] {
            assert!(template[key].is_object(), "{} must be an object", key);
        }
        assert!(template["mission_list"].is_array());
        assert_eq!(template["score_ranking"]["all_rank"], 0);
        assert_eq!(template["member_ranking"]["master_character_id"], 0);
        assert_eq!(template["lottery_box"]["draw_count_list"], array![]);
    }

    #[test]
    fn stored_blobs_with_the_old_array_shapes_are_coerced() {
        // Exactly what accounts created before the template fix have on disk.
        let mut stored = object! {
            point_ranking: object! { point: 12 },
            score_ranking: array![],
            member_ranking: array![],
            lottery_box: array![],
            mission_list: array![],
        };
        normalise_event_shape(&mut stored);

        assert!(stored["score_ranking"].is_object());
        assert_eq!(stored["score_ranking"]["all_rank"], 0);
        assert!(stored["member_ranking"].is_object());
        assert!(stored["lottery_box"].is_object());
        assert_eq!(stored["lottery_box"]["draw_count_list"], array![]);
        // A key that was already there survives; the missing sibling is filled in.
        assert_eq!(stored["point_ranking"]["point"], 12);
        assert_eq!(stored["point_ranking"]["rank"], 0);
        // The multi trio EventData.SetMultiParameter reads.
        assert_eq!(stored["is_disconnected"], 0);
        assert_eq!(stored["help_count"], 0);
        assert_eq!(stored["penalty_remaining_time"], 0);
    }

    #[test]
    fn well_formed_data_is_left_alone() {
        let mut live = object! {
            point_ranking: object! { rank: 3, point: 900 },
            score_ranking: object! { all_rank: 7, group_rank: 2, score: 4242 },
            member_ranking: object! { master_character_id: 5, rank: 1, point: 10 },
            lottery_box: object! { master_lottery_id: 8, reset_count: 1, draw_count_list: array![] },
            mission_list: array![],
            is_disconnected: 1,
            help_count: 4,
            penalty_remaining_time: 600,
        };
        let before = live.clone();
        normalise_event_shape(&mut live);
        assert_eq!(live, before);
    }

    // give_event_points and get_points must agree on what identifies a row. They did not:
    // the write matched on the point type alone, so an account that had ever earned points
    // in ANY event credited every later event to that first row — and get_points, which
    // does compare the event id, then reported 0 for the event actually played.
    #[test]
    fn event_points_land_in_the_row_for_that_event() {
        let mut user = object!{ event_point_list: array![] };

        give_event_points(108, 35, &mut user);
        assert_eq!(get_points(108, &user), 35);

        // A second event opens a row of its own and leaves the first one alone.
        give_event_points(111, 70, &mut user);
        assert_eq!(get_points(111, &user), 70);
        assert_eq!(get_points(108, &user), 35);
        assert_eq!(user["event_point_list"].len(), 2);

        // And a second live in the first event still adds to the first row.
        give_event_points(108, 35, &mut user);
        assert_eq!(get_points(108, &user), 70);
        assert_eq!(get_points(111, &user), 70);
        assert_eq!(user["event_point_list"].len(), 2);

        // An event never played is worth nothing, not somebody else's total.
        assert_eq!(get_points(115, &user), 0);
    }
}

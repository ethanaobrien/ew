use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest};
use crate::router::{chat, global, items, userdata, databases, Session, Api};
use databases::csv::{table, Region};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.route("/story/read", web::post().to(read));
}

static READ_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn has_read(user: &JsonValue, story: i64, part: i64) -> bool {
    user["story_list"].members().any(|s| s["master_story_id"] == story
        && s["master_story_part_ids"].contains(part))
}

pub fn refresh(user: &JsonValue, missions: &mut JsonValue) {
    let mut counts = std::collections::HashMap::<i64, i64>::new();
    for live in user["live_list"].members() {
        let song = &databases::MUSIC[live["master_live_id"].to_string()];
        if !song.is_null() {
            *counts.entry(song["bandCategory"].as_i64().unwrap_or(0)).or_default()
                += live["clear_count"].as_i64().unwrap_or(0);
        }
    }
    for (_, master) in databases::MISSION_LIST.entries() {
        let condition = master["conditionType"].as_i64().unwrap_or(0);
        if !matches!(condition, 73 | 78) { continue; }
        let id = master["id"].as_i64().unwrap();
        let value = master["conditionValues"][0].as_i64().unwrap_or(0);
        let count = if condition == 73 { counts.get(&value).copied().unwrap_or(0) }
            else { super::event::get_points(value as u32, user) };
        if condition == 78 && count == 0 { continue; }
        set_progress(id, count, missions);
    }
}

fn set_progress(id: i64, count: i64, missions: &mut JsonValue) {
    if !missions.members().any(|m| m["master_mission_id"] == id) {
        missions.push(object! {master_mission_id: id, status: 1, progress: 0, expire_date_time: 0, not_visible: 0}).unwrap();
    }
    let master = &databases::MISSION_LIST[id.to_string()];
    let target = master["conditionNumber"].as_i64().unwrap();
    let mission = missions.members_mut().find(|m| m["master_mission_id"] == id).unwrap();
    if mission["status"].as_i64().unwrap_or(1) >= 2 { return; }
    let progress = count.max(mission["progress"].as_i64().unwrap_or(0)).min(target);
    mission["progress"] = progress.into();
    mission["status"] = if progress >= target { 2 } else { 1 }.into();
}

pub fn visit_event(key: &str, event: u32) {
    let user = userdata::get_acc(key);
    let now = global::set_time(global::timestamp(), user["user"]["id"].as_i64().unwrap(), true);
    if !super::event::is_in_session(event, now) { return; }
    let mut missions = userdata::get_acc_missions(key);
    for (_, master) in databases::MISSION_LIST.entries() {
        if master["conditionType"] == 79 && master["conditionValues"][0] == event {
            set_progress(master["id"].as_i64().unwrap(), 1, &mut missions);
        }
    }
    refresh(&user, &mut missions);
    userdata::save_acc_missions(key, missions);
}

async fn read(req: HttpRequest, Session { key, body }: Session) -> Api {
    let _read = crate::lock_onto_mutex!(READ_LOCK);
    let Some(part_id) = body["master_story_part_id"].as_i64() else { return Api(None); };
    let part = &databases::STORY[part_id.to_string()];
    let Some(story_id) = part["masterStoryId"].as_i64() else { return Api(None); };
    let mut user = userdata::get_acc(&key);
    let empty = || object! {gift_list: [], updated_value_list: [], reward_list: [], clear_mission_ids: []};
    if has_read(&user, story_id, part_id) { return Api(Some(empty())); }
    if let Some(previous) = databases::STORY.entries().map(|(_, p)| p)
        .filter(|p| p["masterStoryId"] == story_id && p["number"].as_i64() < part["number"].as_i64())
        .max_by_key(|p| p["number"].as_i64()) {
        if !has_read(&user, story_id, previous["id"].as_i64().unwrap()) { return Api(None); }
    }
    let mut missions = userdata::get_acc_missions(&key);
    refresh(&user, &mut missions);
    let releases = table(Region::Jp, "story_release");
    let now = global::set_time(global::timestamp(), user["user"]["id"].as_i64().unwrap(), true);
    let stories = table(Region::Jp, "story");
    let Some(story_master) = stories.members().find(|s| s["id"] == story_id) else { return Api(None); };
    let mut fully_open = false;
    if let Some(release) = releases.members().find(|r| r["masterStoryPartId"] == part_id) {
        fully_open = global::parse_datetime(release["openedAtAfterEvent"].as_str().unwrap_or(""))
            .is_some_and(|at| now >= at);
        if story_master["type"] == 3 {
            let event_stories = table(Region::Jp, "event_story");
            let ended = event_stories.members().find(|e| e["masterStoryId"] == story_id)
                .and_then(|e| {
                    let event = &databases::EVENTS[e["masterEventId"].to_string()];
                    global::parse_datetime(databases::RELEASE_LABEL[event["masterReleaseLabelId"].to_string()]["closedAt"].as_str().unwrap_or(""))
                }).is_some_and(|end| now > end);
            fully_open &= ended;
        }
        let mission = release["masterMissionId"].as_i64().unwrap_or(0);
        if !fully_open {
            let rank = items::get_user_rank_data(user["user"]["exp"].as_i64().unwrap_or(0))["rank"].as_i64().unwrap_or(0);
            if rank < release["userRank"].as_i64().unwrap_or(0) { return Api(None); }
            let live = release["masterLiveId"].as_i64().unwrap_or(0);
            let score = release["liveScore"].as_i64().unwrap_or(0);
            if live != 0 && score != 0 && !user["live_list"].members().any(|l|
                l["master_live_id"] == live && l["clear_count"].as_i64().unwrap_or(0) > 0
                && l["high_score"].as_i64().unwrap_or(0) >= score) { return Api(None); }
        }
        if !fully_open && mission != 0 && !missions.members().any(|m|
            m["master_mission_id"] == mission && m["status"].as_i64().unwrap_or(0) >= 2) {
            return Api(None);
        }
    }
    if !fully_open && (!super::event::release_label_is_open(part["masterReleaseLabelId"].as_i64().unwrap_or(0), now)
        || !super::event::release_label_is_open(story_master["masterReleaseLabelId"].as_i64().unwrap_or(0), now)) {
        return Api(None);
    }
    let mut home = userdata::get_acc_home(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut response = empty();
    let mut rooms = array![];
    let mut chapters = array![];
    let region = if items::get_region(req.headers()) { Region::Jp } else { Region::En };
    let mut rewards: Vec<_> = table(region, "story_reward").members()
        .filter(|r| r["id"] == part["masterStoryRewardId"]).cloned().collect();
    // Official replies group room rewards before chapter rewards.
    rewards.sort_by_key(|r| (r["type"].as_i64(), r["number"].as_i64()));
    for reward in rewards {
        let value = reward["value"].as_i64().unwrap();
        match reward["type"].as_i64().unwrap() {
            1 => {
                if home["home"]["gift_list"].len() >= items::GIFT_LIMIT { return Api(None); }
                let mut gift = reward.clone();
                let next_id = loop {
                    let id = rand::random::<u64>() & ((1u64 << 53) - 1);
                    if id != 0 && !home["home"]["gift_list"].members().any(|g| g["id"] == id) { break id; }
                };
                gift["id"] = next_id.into();
                let reason = if region == Region::Jp { "スクールアイドルの日常の報酬です。" } else { "Daily Life of School idols reward." };
                let gift = items::gift_item(&gift, reason, &mut home);
                response["gift_list"].push(gift).unwrap();
            }
            16 => {} // Chapter rewards below materialize room ownership.
            17 => {
                if !chat::add_chat_from_chapter_id(value, &mut chats) { continue; }
                chapters.push(value).unwrap();
                let chapter = &databases::CHAPTERS_MASTER[value.to_string()];
                let room = databases::CHATS[chapter["masterChatId"].to_string()][chapter["roomId"].to_string()]["id"].clone();
                rooms.push(room).unwrap();
            }
            _ => return Api(None),
        }
        response["reward_list"].push(object! {
            type: reward["type"].clone(), value: value,
            level: reward["level"].clone(), amount: reward["amount"].clone()
        }).unwrap();
    }
    if !chapters.is_empty() {
        response["updated_value_list"] = object! {master_chat_room_ids: rooms, master_chat_chapter_ids: chapters};
    }
    if !user["story_list"].members().any(|s| s["master_story_id"] == story_id) {
        user["story_list"].push(object! {master_story_id: story_id, master_story_part_ids: []}).unwrap();
    }
    for story in user["story_list"].members_mut().filter(|s| s["master_story_id"] == story_id) {
        story["master_story_part_ids"].push(part_id).unwrap();
    }
    userdata::save_acc_home(&key, home);
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_missions(&key, missions);
    userdata::save_acc(&key, user);
    Api(Some(response))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[actix_web::test]
    async fn event_entry_and_point_thresholds_unlock_story_in_its_original_period() {
        let _test = crate::runtime::lock_test_data_path();
        let (_, key) = userdata::starter::create("Event story test").unwrap();
        userdata::save_server_data(&key, object! {
            server_time: global::parse_datetime("2023/04/22 12:00:00").unwrap(), server_time_set: global::timestamp()
        });
        let request = || actix_web::test::TestRequest::default().to_http_request();
        let part = |id: i64| Session {key: key.clone(), body: object! {master_story_part_id: id}};
        assert!(read(request(), part(40001001)).await.0.is_none());
        visit_event(&key, 101);
        assert!(read(request(), part(40001001)).await.0.is_some());
        let mut user = userdata::get_acc(&key);
        super::super::event::give_event_points(101, 4999, &mut user);
        userdata::save_acc(&key, user);
        assert!(read(request(), part(40001002)).await.0.is_none());
        let mut user = userdata::get_acc(&key);
        super::super::event::give_event_points(101, 1, &mut user);
        userdata::save_acc(&key, user);
        assert!(read(request(), part(40001002)).await.0.is_some());
        assert!(read(request(), part(40001003)).await.0.is_none());
    }

    #[actix_web::test]
    async fn story_unlocks_before_and_after_full_release() {
        let _test = crate::runtime::lock_test_data_path();
        let (_, key) = userdata::starter::create("Story unlock test").unwrap();
        let request = || actix_web::test::TestRequest::default().to_http_request();
        let read_part = |id: i64| Session {key: key.clone(), body: object! {master_story_part_id: id}};
        userdata::save_server_data(&key, object! {
            server_time: global::parse_datetime("2023/12/01 12:00:00").unwrap(), server_time_set: global::timestamp()
        });
        assert!(read(request(), read_part(210001001)).await.0.is_none());
        let mut user = userdata::get_acc(&key);
        user["live_list"] = array![object! {master_live_id: 1100101, clear_count: 1}];
        userdata::save_acc(&key, user);
        assert!(read(request(), read_part(210001001)).await.0.is_some());
        assert!(read(request(), read_part(210001002)).await.0.is_none());
        let mut user = userdata::get_acc(&key);
        user["live_list"][0]["clear_count"] = 10.into();
        userdata::save_acc(&key, user);
        assert!(read(request(), read_part(210001002)).await.0.is_some());
        // Event parts remain gated before full release, including previous-part order.
        assert!(read(request(), read_part(40001001)).await.0.is_none());
        userdata::save_server_data(&key, object! {
            server_time: global::parse_datetime("2024/03/01 12:00:00").unwrap(), server_time_set: global::timestamp()
        });
        assert!(read(request(), read_part(40001002)).await.0.is_none());
        assert!(read(request(), read_part(40001001)).await.0.is_some());
        assert!(read(request(), read_part(40001002)).await.0.is_some());
    }

    #[actix_web::test]
    async fn story_rewards_persist_and_replays_do_not_pay_again() {
        let _test = crate::runtime::lock_test_data_path();
        let (_, key) = userdata::starter::create("Story test").unwrap();
        userdata::save_acc_chats(&key, array![]);
        let request = || actix_web::test::TestRequest::default().to_http_request();
        let reply = read(request(), Session {key: key.clone(), body: object! {master_story_part_id: 20000001}}).await.0.unwrap();
        // jp/logs.txt:1871-1902, the first Aqours introduction read.
        assert_eq!(reply["updated_value_list"], object! {
            master_chat_room_ids: [2001001, 2101001], master_chat_chapter_ids: [200100101, 210100101]
        });
        assert_eq!(reply["reward_list"].len(), 4);
        assert_eq!(reply["reward_list"][0]["type"], 16);
        assert_eq!(userdata::get_acc_chats(&key).len(), 2);
        assert!(has_read(&userdata::get_acc(&key), 129999, 20000001));
        let repeat = read(request(), Session {key: key.clone(), body: object! {master_story_part_id: 20000001}}).await.0.unwrap();
        assert!(repeat["reward_list"].is_empty());

        // jp/logs.txt:18507-18538: daily-life gems go to gifts, not straight to the wallet.
        let mut user = userdata::get_acc(&key);
        user["live_list"] = array![object! {master_live_id: 1100101, clear_count: 1}];
        userdata::save_acc(&key, user);
        let gems = userdata::get_acc(&key)["gem"].clone();
        let before = userdata::get_acc_home(&key)["home"]["gift_list"].len();
        let first = read(request(), Session {key: key.clone(), body: object! {master_story_part_id: 210001001}}).await.0.unwrap();
        assert_eq!(first["gift_list"].len(), 1);
        assert_eq!(first["gift_list"][0]["amount"], 100);
        assert_eq!(userdata::get_acc_home(&key)["home"]["gift_list"].len(), before + 1);
        assert_eq!(userdata::get_acc(&key)["gem"], gems);
        let repeat = read(request(), Session {key: key.clone(), body: object! {master_story_part_id: 210001001}}).await.0.unwrap();
        assert!(repeat["gift_list"].is_empty());
        assert_eq!(userdata::get_acc_home(&key)["home"]["gift_list"].len(), before + 1);
        assert!(read(request(), Session {key, body: object! {master_story_part_id: -1}}).await.0.is_none());
    }
}

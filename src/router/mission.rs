use jzon::{array, object, JsonValue};
use actix_web::{web, HttpRequest, Responder};

use crate::router::{global, userdata, items, databases, Login, Session, Api};

pub fn filter_response(data: &mut JsonValue, headers: &actix_web::http::header::HeaderMap) {
    if items::get_region(headers) { return; }
    lazy_static::lazy_static! {
        static ref GLOBAL_IDS: std::collections::HashSet<i64> =
            databases::csv::table(databases::csv::Region::En, "mission")
                .members().filter_map(|row| row["id"].as_i64()).collect();
    }
    // Only project the response. Shared JP/global accounts keep their progress.
    for field in ["mission_list", "clear_mission_ids"] {
        if !data[field].is_array() { continue; }
        for i in (0..data[field].len()).rev() {
            let id = if field == "mission_list" { data[field][i]["master_mission_id"].as_i64() }
                else { data[field][i].as_i64() };
            if !id.is_some_and(|id| GLOBAL_IDS.contains(&id)) {
                data[field].array_remove(i);
            }
        }
    }
    if data["updated_value_list"].is_object() {
        filter_response(&mut data["updated_value_list"], headers);
    }
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/mission")
            .route("", web::get().to(mission))
            .route("/clear", web::post().to(clear))
            .route("/receive", web::post().to(receive))
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[actix_web::test]
    async fn mission_endpoint_filters_global_wire_response_not_saved_account() {
        let _test = crate::runtime::lock_test_data_path();
        let (uid, key) = userdata::starter::create("Regional missions test").unwrap();
        let mut user = userdata::get_acc(&key);
        user["tutorial_step"] = 999.into();
        userdata::save_acc(&key, user);
        for version in ["5260ff15dff8ba0c00ad91400f515f55", "4c921d2443335e574a82e04ec9ea243c"] {
            let request = actix_web::test::TestRequest::default()
                .insert_header(("aoharu-user-id", uid.to_string()))
                .insert_header(("aoharu-asset-version", version)).to_http_request();
            let response = mission(Login(key.clone())).await.respond_to(&request);
            let body = actix_web::body::to_bytes(response.into_body()).await.ok().unwrap();
            let plain = crate::encryption::decrypt_packet(std::str::from_utf8(&body).unwrap()).unwrap();
            let response = jzon::parse(&plain).unwrap();
            assert_eq!(response["code"], 0);
            let missions = &response["data"]["mission_list"];
            assert!(missions.members().any(|m| m["master_mission_id"] == 1073001));
            assert_eq!(missions.members().any(|m| m["master_mission_id"] == 1073037),
                items::get_region(request.headers()));
            assert!(userdata::get_acc_missions(&key).members()
                .any(|m| m["master_mission_id"] == 1073037));
        }
    }

    #[test]
    fn global_missions_match_client_masterdata_without_losing_saved_progress() {
        let request = actix_web::test::TestRequest::default()
            .insert_header(("aoharu-asset-version", "5260ff15dff8ba0c00ad91400f515f55"))
            .to_http_request();
        assert!(!items::get_region(request.headers()));
        let mut saved = array![];
        super::super::story::refresh(&object! {live_list: []}, &mut saved);
        let original = saved.clone();
        let ids = databases::csv::table(databases::csv::Region::En, "mission");
        let known: std::collections::HashSet<_> = ids.members()
            .filter_map(|r| r["id"].as_i64()).collect();
        let absent: Vec<_> = saved.members().filter_map(|m| {
            let id = m["master_mission_id"].as_i64().unwrap();
            (!known.contains(&id)).then_some(id)
        }).collect();
        assert_eq!(absent.len(), 30);
        assert!(absent.iter().all(|id| (1073037..=1073066).contains(id)));
        let expected: JsonValue = saved.members().filter(|m|
            known.contains(&m["master_mission_id"].as_i64().unwrap()))
            .cloned().collect::<Vec<_>>().into();
        let mut response = object! {mission_list: saved.clone(),
            clear_mission_ids: [1073037, 1073001, 1073066],
            updated_value_list: {mission_list: saved.clone(), clear_mission_ids: [1073001, 1073037]},
            reward_list: [{type: 1, amount: 100}]};
        filter_response(&mut response, request.headers());
        assert_eq!(response["mission_list"], expected);
        assert_eq!(response["updated_value_list"]["mission_list"], expected);
        assert_eq!(response["clear_mission_ids"], array![1073001]);
        assert_eq!(response["updated_value_list"]["clear_mission_ids"], array![1073001]);
        assert_eq!(response["reward_list"], array![object! {type: 1, amount: 100}]);
        assert_eq!(saved, original);
        let mut unrelated = object! {item_list: [], value: 42};
        let before = unrelated.clone();
        filter_response(&mut unrelated, request.headers());
        assert_eq!(unrelated, before);
        let mut jp = object! {mission_list: saved};
        let before = jp.clone();
        filter_response(&mut jp, actix_web::test::TestRequest::default().to_http_request().headers());
        assert_eq!(jp, before);
    }

    #[actix_web::test]
    async fn bond_title_claim_notifies_client_and_persists() {
        let _test = crate::runtime::lock_test_data_path();
        let (uid, key) = userdata::starter::create("Bond title test").unwrap();
        let request = || actix_web::test::TestRequest::default()
            .insert_header(("aoharu-user-id", uid.to_string())).to_http_request();
        let initial_titles = userdata::get_acc(&key)["master_title_ids"].clone();
        userdata::save_acc_missions(&key, array![object! {
            master_mission_id: 1158015, status: 2, progress: 1500, expire_date_time: 0
        }]);

        let response = receive(request(), Session {
            key: key.clone(), body: object! {master_mission_ids: [1158015, 1158015]}
        }).await.0.unwrap();
        assert_eq!(response["reward_list"].len(), 1);
        assert_eq!(response["reward_list"][0]["type"], 8);
        assert_eq!(response["reward_list"][0]["value"], 3000015);
        assert_eq!(response["updated_value_list"]["master_title_ids"], array![3000015]);
        assert!(response["updated_value_list"]["item_list"].is_null());
        let saved = userdata::get_acc(&key);
        assert!(saved["master_title_ids"].contains(3000015));
        for title in initial_titles.members() {
            assert!(saved["master_title_ids"].contains(title.as_i64().unwrap()));
        }

        let replay = receive(request(), Session {
            key: key.clone(), body: object! {master_mission_ids: [1158015]}
        }).await.0.unwrap();
        assert!(replay["reward_list"].is_empty());
        assert!(replay["updated_value_list"]["master_title_ids"].is_null());
        assert_eq!(userdata::get_acc(&key), saved);

        let mut missions = userdata::get_acc_missions(&key);
        items::update_mission_status(1158054, 0, true, false, 13500, &mut missions);
        userdata::save_acc_missions(&key, missions);
        let next = receive(request(), Session {
            key: key.clone(), body: object! {master_mission_ids: [1158054]}
        }).await.0.unwrap();
        assert_eq!(next["updated_value_list"]["master_title_ids"], array![3000054]);
        let saved = userdata::get_acc(&key);
        assert!(saved["master_title_ids"].contains(3000015));
        assert!(saved["master_title_ids"].contains(3000054));
    }

    #[actix_web::test]
    async fn beginner_claims_persist_rewards_and_reject_repeats() {
        let _test = crate::runtime::lock_test_data_path();
        let (uid, key) = userdata::starter::create("Beginner test").unwrap();
        let request = || actix_web::test::TestRequest::default()
            .insert_header(("aoharu-user-id", uid.to_string())).to_http_request();
        let first = [1653001,1605001,1662001,1663001,1660001,1614001,1666001,1658001,1661001];
        let mut missions = userdata::get_acc_missions(&key);
        for id in first {
            super::super::beginner_mission::set_progress(id, i64::MAX, &mut missions);
        }
        super::super::beginner_mission::set_progress(1653002, 2, &mut missions);
        userdata::save_acc_missions(&key, missions);
        let blocked = receive(request(), Session { key: key.clone(), body: object! {master_mission_ids: [1653002]} }).await.0.unwrap();
        assert!(blocked["reward_list"].is_empty());
        let gem_before = userdata::get_acc(&key)["gem"]["total"].as_i64().unwrap();
        let mut ids: Vec<i64> = first.into();
        ids.push(first[0]);
        let body = object! {master_mission_ids: ids};
        let response = receive(request(), Session { key: key.clone(), body: body.clone() }).await.0.unwrap();
        assert_eq!(response["mission_list"].len(), 9);
        assert_eq!(response["reward_list"].len(), 18);
        let bonuses: i64 = response["reward_list"].members().filter(|r| r["type"] == 1)
            .map(|r| r["amount"].as_i64().unwrap()).sum();
        assert_eq!(bonuses, 1000);
        assert_eq!(userdata::get_acc(&key)["gem"]["total"].as_i64().unwrap(), gem_before + bonuses);
        let items = &response["updated_value_list"]["item_list"];
        let unique: std::collections::HashSet<_> = items.members().map(|i| i["master_item_id"].as_i64()).collect();
        assert_eq!(items.len(), unique.len());
        let saved = userdata::get_acc(&key);
        let replay = receive(request(), Session { key: key.clone(), body }).await.0.unwrap();
        assert!(replay["reward_list"].is_empty());
        assert_eq!(userdata::get_acc(&key), saved);
        let second = receive(request(), Session { key: key.clone(), body: object! {master_mission_ids: [1653002, 1605002]} }).await.0.unwrap();
        assert_eq!(second["mission_list"].len(), 1);
        assert_eq!(second["mission_list"][0]["master_mission_id"], 1653002);
        assert_eq!(second["mission_list"][0]["progress"], 2);
    }
}

const VARIABLE_MISSIONS: [[i64; 2]; 5] = [[1153001, 1153019], [1105001, 1105017], [1101001, 1101030], [1121001, 1121019], [1112001, 1112033]];

async fn mission(Login(key): Login) -> impl Responder {
    let user = userdata::get_acc(&key);
    let step = user["tutorial_step"].as_i64().unwrap_or(0);

    if (1..130).contains(&step) {
        return Api(Some(object!{
            "mission_list": array![]
        }));
    }

    let mut missions = userdata::get_acc_missions(&key);
    items::refresh_dailies(&mut missions, global::timestamp());
    super::story::refresh(&user, &mut missions);
    super::beginner_mission::refresh_account(&user, &mut missions);
    userdata::save_acc_missions(&key, missions.clone());

    Api(Some(object!{
        "mission_list": missions
    }))
}

async fn clear(Session { key, body }: Session) -> impl Responder {
    let mut missions = userdata::get_acc_missions(&key);

    let mut cleared = array![];
    for id in body["master_mission_ids"].members() {
        if super::beginner_mission::contains(id.as_i64().unwrap_or(0)) {
            let condition = databases::MISSION_LIST[id.to_string()]["conditionType"].as_i64();
            if !matches!(condition, Some(26 | 65 | 66)) { continue; }
        }
        let mission = items::get_mission_status(id.as_i64().unwrap(), &missions);
        if mission.is_empty() || mission["status"].as_i32().unwrap_or(0) >= 2 {
            continue;
        }
        items::update_mission_status(id.as_i64().unwrap(), 0, true, false, 1, &mut missions);
        cleared.push(id.clone()).unwrap();
    }

    userdata::save_acc_missions(&key, missions);

    Api(Some(object!{
        "clear_mission_ids": cleared
    }))
}

static CLAIM_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

async fn receive(req: HttpRequest, Session { key, body }: Session) -> Api {
    // Keep the read/grant/save sequence exclusive across HTTP workers.
    let _claim = crate::lock_onto_mutex!(CLAIM_LOCK);
    let uid = global::get_uid(req.headers());
    let now = global::timestamp();

    let mut missions = userdata::get_acc_missions(&key);
    let mut user = userdata::get_acc(&key);
    let mut chats = userdata::get_acc_chats(&key);
    let mut rewards = array![];
    let mut mission_list = array![];
    let mut touched_gem = false;
    let mut touched_coin = false;
    let mut touched_items = array![];
    let mut touched_titles = std::collections::BTreeSet::new();
    let before = missions.clone();
    let mut requested: Vec<_> = body["master_mission_ids"].members().filter_map(|id| id.as_i64()).collect();
    let mut seen = std::collections::HashSet::new();
    requested.retain(|id| seen.insert(*id));
    // Process beginner cells in sheet order so a single claim can unlock the next sheet.
    requested.sort_by_key(|id| super::beginner_mission::level(*id));

    for mid in requested {
        if items::get_mission_status(mid, &missions)["status"] != 2 {
            continue;
        }
        if super::beginner_mission::contains(mid) && !super::beginner_mission::claimable(mid, &missions) {
            continue;
        }
        let mission_info = databases::MISSION_LIST[mid.to_string()].clone();
        for master in databases::MISSION_REWARDS[mission_info["masterMissionRewardId"].to_string()].members() {
            let reward_type = master["type"].as_i64().unwrap();

            rewards.push(object!{
                give_type: master["giveType"].clone(),
                type: master["type"].clone(),
                value: master["value"].clone(),
                level: master["level"].clone(),
                amount: master["amount"].clone()
            }).unwrap();

            items::give_gift(&object!{
                reward_type: reward_type,
                value: master["value"].clone(),
                amount: master["amount"].clone()
            }, &mut user, &mut missions, &mut array![], &mut chats);

            match reward_type {
                1 => touched_gem = true,
                4 => touched_coin = true,
                8 => { touched_titles.insert(master["value"].as_i64().unwrap()); }
                _ => { touched_items.push(master["value"].clone()).unwrap(); }
            }
        }

        let mut variable = false;
        for range in VARIABLE_MISSIONS {
            if mid >= range[0] && mid < range[1] {
                items::change_mission_id(mid, mid + 1, &mut missions);
                let next = databases::MISSION_LIST[(mid + 1).to_string()]["conditionNumber"].as_i64().unwrap_or(0);
                for m in missions.members_mut() {
                    if m["master_mission_id"].as_i64() == Some(mid + 1) {
                        let progress = m["progress"].as_i64().unwrap_or(0);
                        m["status"] = if progress >= next { 2 } else { 1 }.into();
                        mission_list.push(m.clone()).unwrap();
                        break;
                    }
                }
                variable = true;
                break;
            }
        }
        if !variable && (1158001..=1158039).contains(&mid) {
            items::change_mission_id(mid, mid + 39, &mut missions);
            items::update_mission_status(mid + 39, 0, false, false, 0, &mut missions);
            for m in missions.members_mut() {
                if m["master_mission_id"].as_i64() == Some(mid + 39) {
                    mission_list.push(m.clone()).unwrap();
                    break;
                }
            }
            variable = true;
        }
        if !variable {
            for m in missions.members_mut() {
                if m["master_mission_id"].as_i64() == Some(mid) {
                    m["status"] = (3).into();
                    break;
                }
            }
            let m = items::get_mission_status(mid, &missions);
            let expire = m["expire_date_time"].as_u64().unwrap_or(0);
            mission_list.push(object!{
                user_id: uid,
                master_mission_id: mid,
                status: 3,
                progress: if expire != 0 || super::beginner_mission::is_counter(mid) { m["progress"].clone() } else { JsonValue::Null },
                expire_date: if expire != 0 { global::format_datetime(expire).into() } else { JsonValue::Null },
                received_date: global::format_datetime(now)
            }).unwrap();
        }
    }

    for master in super::beginner_mission::new_rewards(&before, &missions) {
        let reward_type = master["type"].as_i64().unwrap();
        rewards.push(object! {
            give_type: master["giveType"].clone(), type: master["type"].clone(),
            value: master["value"].clone(), level: master["level"].clone(), amount: master["amount"].clone()
        }).unwrap();
        items::give_gift(&object! {
            reward_type: reward_type, value: master["value"].clone(), amount: master["amount"].clone()
        }, &mut user, &mut missions, &mut array![], &mut chats);
        match reward_type {
            1 => touched_gem = true,
            4 => touched_coin = true,
            8 => { touched_titles.insert(master["value"].as_i64().unwrap()); }
            _ => { touched_items.push(master["value"].clone()).unwrap(); }
        }
    }
    userdata::save_acc(&key, user.clone());
    userdata::save_acc_chats(&key, chats);
    userdata::save_acc_missions(&key, missions);

    let mut updated_value_list = object!{};
    if !touched_titles.is_empty() {
        updated_value_list["master_title_ids"] = touched_titles.into_iter().collect::<Vec<_>>().into();
    }
    if touched_gem {
        updated_value_list["gem"] = user["gem"].clone();
    }
    if !touched_items.is_empty() {
        let mut item_list = array![];
        let mut seen = std::collections::HashSet::new();
        for id in touched_items.members() {
            if !seen.insert(id.as_i64()) { continue; }
            for item in user["item_list"].members() {
                if item["master_item_id"] == *id {
                    item_list.push(item.clone()).unwrap();
                    break;
                }
            }
        }
        updated_value_list["item_list"] = item_list;
    }
    if touched_coin {
        let mut point_list = array![];
        for point in user["point_list"].members() {
            if point["type"].as_i64() == Some(1) {
                point_list.push(object!{
                    type: 1,
                    amount: point["amount"].clone()
                }).unwrap();
                break;
            }
        }
        updated_value_list["point_list"] = point_list;
    }

    let mut sorted: Vec<_> = mission_list.members().cloned().collect();
    sorted.sort_by_key(|m| m["master_mission_id"].as_i64());
    let mission_list: JsonValue = sorted.into();
    Api(Some(object!{
        "reward_list": rewards,
        "gift_list": array![],
        "updated_value_list": updated_value_list,
        "mission_list": mission_list
    }))
}

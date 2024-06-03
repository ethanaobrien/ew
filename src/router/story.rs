use json::{object, JsonValue};
use actix_web::{HttpRequest};

use crate::encryption;
use crate::router::{global, userdata, databases};

pub fn read(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc(&key);
    let part = body["master_story_part_id"].as_i64().unwrap();
    let master_id = databases::STORY[part.to_string()]["masterStoryId"].as_i64().unwrap();

    let index = user["story_list"].members().position(|r| r["master_story_id"] == master_id);

    if index.is_none() {
        user["story_list"].push(object!{
            master_story_id: master_id,
            master_story_part_ids: []
        }).unwrap();
    }

    for story in user["story_list"].members_mut() {
        if story["master_story_id"] == master_id && !story["master_story_part_ids"].contains(part) {
            story["master_story_part_ids"].push(part).unwrap();
        }
    }

    userdata::save_acc(&key, user.clone());


    Some(object!{
        "gift_list":[],
        "updated_value_list":{
            "story_list": user["story_list"].clone()
        },
        "reward_list":[],
        "clear_mission_ids":[]
    })
}

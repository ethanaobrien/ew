use json::{object, JsonValue};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::global;

pub fn read(req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(object!{"gift_list":[],"updated_value_list":{"master_chat_room_ids":[3001001,3101001],"master_chat_chapter_ids":[300100101,310100101]},"reward_list":[{"type":16,"value":3001001,"level":0,"amount":1},{"type":16,"value":3101001,"level":0,"amount":1},{"type":17,"value":300100101,"level":0,"amount":1},{"type":17,"value":310100101,"level":0,"amount":1}],"clear_mission_ids":[]})
}

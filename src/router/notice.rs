use json::{object, array, JsonValue};
use actix_web::{HttpRequest};


//todo
pub fn reward(_req: HttpRequest) -> Option<JsonValue> {
    Some(object!{
        "reward_list": []
    })
}

pub fn reward_post(_req: HttpRequest, _body: String) -> Option<JsonValue> {
    Some(array![])
}

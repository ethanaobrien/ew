use json::{object, JsonValue};
use actix_web::{HttpRequest};

pub fn location(_req: HttpRequest) -> Option<JsonValue> {
    Some(object!{
        "master_location_ids": []
    })
}

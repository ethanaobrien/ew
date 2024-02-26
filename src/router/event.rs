use json;
use json::object;
use crate::router::global;
use actix_web::{HttpResponse, HttpRequest};
//use crate::router::userdata;

pub fn event(_req: HttpRequest, _body: String) -> HttpResponse {
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data":{"point_ranking":{"point":0},"score_ranking":[],"member_ranking":[],"lottery_box":[],"mission_list":[],"policy_agreement":0,"incentive_lottery":0,"star_event":{"star_level":0,"last_event_star_level":0,"star_music_list":[],"is_star_event_update":1,"music_change_count":0,"star_event_bonus_daily_count":0,"star_event_bonus_count":0,"star_event_play_times_bonus_count":0,"star_assist_bonus":1}}
    };
    global::send(resp)
}

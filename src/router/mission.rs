use json;
use json::object;
use crate::router::global;
use actix_web::{HttpResponse, HttpRequest};
use crate::router::userdata;
use crate::encryption;

pub fn mission(req: HttpRequest) -> HttpResponse {
    let key = global::get_login(req.headers(), "");
    let missions = userdata::get_acc_missions(&key);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "mission_list": missions
        }
    };
    global::send(resp)
}

pub fn clear(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let mut missions = userdata::get_acc_missions(&key);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    for (_i, id) in body["master_mission_ids"].members().enumerate() {
        for (i, mission) in missions.members().enumerate() {
            if mission["master_mission_id"].to_string() == id.to_string() {
                //I think this is all?
                missions[i]["progress"] = (1).into();
                break;
            }
        }
    }
    
    userdata::save_acc_missions(&key, missions);
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "clear_mission_ids": body["master_mission_ids"].clone()
        }
    };
    global::send(resp)
}

pub fn receive(_req: HttpRequest, _body: String) -> HttpResponse {
    //let key = global::get_login(req.headers(), &body);
    //let missions = userdata::get_acc_missions(&key);
    //let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    
    //todo - give user rewards based off of cleared missions
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "reward_list": []
        }
    };
    global::send(resp)
}

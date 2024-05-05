use json::{array, object};
use actix_web::{HttpResponse, HttpRequest};

use crate::router::{global, userdata, items};
use crate::encryption;

pub fn events(req: HttpRequest) -> HttpResponse {
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "serial_code_list": []
        }
    };
    global::send(resp, req)
}

pub fn serial_code(req: HttpRequest, body: String) -> HttpResponse {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc_home(&key);
    
    let itemz;
    if body["input_code"].to_string() == "SIF2REVIVALREAL!" {
        itemz = array![items::gift_item_basic(1, 100000, 4, "Another game died... This makes me sad :(", &mut user)];
    } else if body["input_code"].to_string() == "pweasegivegems11" {
        itemz = array![items::gift_item_basic(1, 6000, 1, "Only because you asked...", &mut user)];
    } else if body["input_code"].to_string() == "sleepysleepyslep" {
        itemz = array![items::gift_item_basic(15540001, 50, 3, "I am tired", &mut user)];
    } else if body["input_code"].to_string() == "ilikeganyu!!!!!!" {
        itemz = array![items::gift_item_basic(16005003, 100, 3, "I need more primogems", &mut user)];
    } else if body["input_code"].to_string() == "hu tao" {
        itemz = array![
            items::gift_item_basic(15500001, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15500002, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520001, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520002, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520003, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520004, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520005, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520006, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520007, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520008, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520009, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520010, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520011, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520012, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520013, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520014, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520015, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520016, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520017, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520018, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520019, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520020, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510004, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510005, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510006, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510007, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510008, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510009, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510010, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510011, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510012, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510013, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510014, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510015, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510016, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510017, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510018, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510019, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510020, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510021, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510022, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510023, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510024, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530001, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530002, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530003, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530004, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530005, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530006, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530007, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530008, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530009, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530010, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530011, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530012, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530013, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530014, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530015, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530016, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530017, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530018, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530019, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530020, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530021, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530022, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530023, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530024, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530025, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530026, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530027, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530028, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530029, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530030, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530031, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530032, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530033, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530034, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530035, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530036, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530037, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540002, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540005, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540006, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540007, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540008, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540009, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540010, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540011, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540012, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540013, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540014, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540015, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540016, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540017, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540023, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540024, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540025, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540027, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540028, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540029, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540030, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540031, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540032, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540033, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540034, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540035, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010002, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010003, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010004, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010005, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010001, 10, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540001, 10, 3, "Okay...............", &mut user),
        ];
    } else {
        let resp = object!{
            "code": 0,
            "server_time": global::timestamp(),
            "data": {
                "result_code": 3
            }
        };
        return global::send(resp, req);
    }
    
    userdata::save_acc_home(&key, user.clone());
    
    let resp = object!{
        "code": 0,
        "server_time": global::timestamp(),
        "data": {
            "serial_code_event": {"id":42,"name":"Serial Code Reward","unique_limit_count":0,"min_user_rank":0,"max_user_rank":0,"end_date":null},
            "reward_list": itemz,
            "result_code": 0,
            "gift_list": user["gift_list"].clone(),
            "excluded_gift_list": []
        }
    };
    global::send(resp, req)
}

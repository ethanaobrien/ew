use json::{array, object, JsonValue};
use actix_web::{HttpRequest};

use crate::router::{global, userdata, items};
use crate::encryption;

pub fn events(req: HttpRequest) -> Option<JsonValue> {
    Some(object!{
        "serial_code_list": []
    })
}

pub fn serial_code(req: HttpRequest, body: String) -> Option<JsonValue> {
    let key = global::get_login(req.headers(), &body);
    let body = json::parse(&encryption::decrypt_packet(&body).unwrap()).unwrap();
    let mut user = userdata::get_acc_home(&key);
    
    let itemz;
    if body["input_code"] == "SIF2REVIVALREAL!" {
        itemz = array![items::gift_item_basic(1, 100000, 4, "Another game died... This makes me sad :(", &mut user)];
    } else if body["input_code"] == "pweasegivegems11" {
        itemz = array![items::gift_item_basic(1, 6000, 1, "Only because you asked...", &mut user)];
    } else if body["input_code"] == "sleepysleepyslep" {
        itemz = array![items::gift_item_basic(15540001, 50, 3, "I am tired", &mut user)];
    } else if body["input_code"] == "ilikeganyu!!!!!!" {
        itemz = array![items::gift_item_basic(16005003, 100, 3, "I need more primogems", &mut user)];
    } else if body["input_code"] == "serial_code" {
        itemz = array![items::gift_item_basic(17001003, 100, 3, "nyaa~", &mut user)];
    } else if body["input_code"] == "ganuy" {
        itemz = array![
            items::gift_item_basic(40010015, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(30010015, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20010018, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(10040018, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20050016, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(30070015, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(40030013, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(10070016, 1, 2, "I need more primogem!!!!!!", &mut user)
        ];
    } else if body["input_code"] == "kode" {
        itemz = array![
            items::gift_item_basic(10060018, 1, 2, "meow", &mut user),
            items::gift_item_basic(20050019, 1, 2, "meow", &mut user),
            items::gift_item_basic(10020018, 1, 2, "meow", &mut user),
            items::gift_item_basic(10010014, 1, 2, "meow", &mut user),
            items::gift_item_basic(10010015, 1, 2, "meow", &mut user)
        ];
    } else if body["input_code"] == "meow" {
        itemz = array![
            items::gift_item_basic(10010020, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(10040016, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(10050018, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(10080016, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(10090015, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20010019, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20030015, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20050014, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20070013, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20080016, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(20090013, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(30010017, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(30020009, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(30040012, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(30090009, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(40010011, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(40030009, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(40040013, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(40060010, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(40080011, 1, 2, "I need more primogem!!!!!!", &mut user),
            items::gift_item_basic(40090011, 1, 2, "I need more primogem!!!!!!", &mut user)
        ];
    } else if body["input_code"] == "HuTao" {
        itemz = array![
            items::gift_item_basic(15500001, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15500002, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520001, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520002, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520003, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520004, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520005, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520006, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520007, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520008, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520009, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520010, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520011, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520012, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520013, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520014, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520015, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520016, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520017, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520018, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520019, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15520020, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510004, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510005, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510006, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510007, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510008, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510009, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510010, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510011, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510012, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510013, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510014, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510015, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510016, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510017, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510018, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510019, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510020, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510021, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510022, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510023, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15510024, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530001, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530002, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530003, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530004, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530005, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530006, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530007, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530008, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530009, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530010, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530011, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530012, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530013, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530014, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530015, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530016, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530017, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530018, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530019, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530020, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530021, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530022, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530023, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530024, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530025, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530026, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530027, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530028, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530029, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530030, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530031, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530032, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530033, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530034, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530035, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530036, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15530037, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540002, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540005, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540006, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540007, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540008, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540009, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540010, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540011, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540012, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540013, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540014, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540015, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540016, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540017, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540023, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540024, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540025, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540027, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540028, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540029, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540030, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540031, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540032, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540033, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540034, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540035, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010002, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010003, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010004, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010005, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(30010001, 500, 3, "Okay...............", &mut user),
            items::gift_item_basic(15540001, 500, 3, "Okay...............", &mut user),
        ];
    } else {
        return Some(object!{
            "result_code": 3
        });
    }
    
    if body["receive_flg"].as_i32().unwrap_or(1) == 1 {
        userdata::save_acc_home(&key, user.clone());
    }
    
    Some(object!{
        "serial_code_event": {"id":42,"name":"Serial Code Reward","unique_limit_count":0,"min_user_rank":0,"max_user_rank":0,"end_date":null},
        "reward_list": itemz,
        "result_code": 0,
        "gift_list": user["gift_list"].clone(),
        "excluded_gift_list": []
    })
}

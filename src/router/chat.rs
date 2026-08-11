use jzon::{object, array, JsonValue};
use actix_web::{web, Responder};

use crate::router::{global, items, userdata, databases, Login, Session, Api};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/chat")
            .route("/home", web::post().to(home))
            .route("/talk/get_stamp", web::get().to(get_stamp))
            .route("/talk/start", web::post().to(start))
            .route("/talk/end", web::post().to(end))
    );
}

// The stamps this account owns. ew does not track stamp unlocks, so everyone has the
// masterdata initial set — the same list /chat/home reports, deliberately from one
// source so the two endpoints can never disagree.
fn owned_stamp_ids() -> JsonValue {
    databases::INITIAL_CHAT_STAMPS.clone()
}

// GET /api/chat/talk/get_stamp (Protocol.send_get_stamp, FuncId.GET_STAMP).
// RecvGetStampRData carries a single `master_chat_stamp_ids` array, which its Notify()
// feeds to CallOnUpdateChatStampListNotify — the same sink RecvChatHomeRData uses, so
// the stamp picker ends up with whatever this returns. The client sends no parameters.
//
// This endpoint appears in neither official capture (0 hits across the 288MB JP and
// 1.4GB EN logs), so the shape is taken from the client class and the contents from the
// /chat/home captures that do exist and carry the same field.
async fn get_stamp(Login(_key): Login) -> impl Responder {
    Api(Some(object!{
        "master_chat_stamp_ids": owned_stamp_ids()
    }))
}

pub fn add_chat(id: i64, num: i64, chats: &mut JsonValue) -> bool {
    for data in chats.members() {
        if data["chat_id"] == id && data["room_id"] == num {
            return false;
        }
    }
    chats.push(object!{
        chat_id: id,
        room_id: num,
        chapter_id: databases::CHAPTERS[id.to_string()][num.to_string()]["id"].clone(),
        is_read: 0,
        created_at: global::timestamp()
    }).unwrap();
    true
}

pub fn add_chat_from_chapter_id(chapter_id: i64, chats: &mut JsonValue) -> bool {
    let chapter = &databases::CHAPTERS_MASTER[chapter_id.to_string()];
    if chapter.is_empty() {
        println!("Attempted to give unknown chapter id {}", chapter_id);
        return false;
    }
    add_chat(chapter["masterChatId"].as_i64().unwrap(), chapter["roomId"].as_i64().unwrap(), chats)
}

async fn home(Login(key): Login) -> impl Responder {
    let chats = userdata::get_acc_chats(&key);
    
    let mut rooms = array![];
    for data in chats.members() {
        rooms.push(databases::CHATS[data["chat_id"].to_string()][data["room_id"].to_string()]["id"].clone()).unwrap();
    }
    
    Api(Some(object!{
        "progress_list": chats,
        "master_chat_room_ids": rooms,
        "master_chat_stamp_ids": owned_stamp_ids(),
        "master_chat_attachment_ids": []
    }))
}

async fn start() -> impl Responder {
    Api(Some(object!{"select_talk_id_list":[],"get_item_list":[],"is_read":0}))
}

async fn end(Session { key, body }: Session) -> impl Responder {
    let mut missions = userdata::get_acc_missions(&key);
    let mut chats = userdata::get_acc_chats(&key);
    
    for data in chats.members_mut() {
        if body["chapter_id"].as_i64().unwrap() == data["chapter_id"].as_i64().unwrap() {
            if data["is_read"].as_i32().unwrap() != 1 {
                items::advance_mission(1169001, 1, 50, &mut missions);
            }
            data["is_read"] = 1.into();
            userdata::save_acc_missions(&key, missions);
            userdata::save_acc_chats(&key, chats);
            break;
        }
    }
    
    Api(Some(array![]))
}

#[cfg(test)]
mod tests {
    use super::*;

    // Verbatim from an official /api/chat/home capture (JP log, the 65-occurrence
    // baseline seen on accounts that had earned no extra stamps yet). Both the JP and EN
    // chat_stamp tables reproduce it exactly from _initialStamp, which is what lets
    // get_stamp serve masterdata instead of a hardcoded literal.
    const OFFICIAL_INITIAL_STAMPS: [i64; 97] = [
        1,2,3,4,5,6,7,8,9,10,11,12,13,14,15,16,17,19,20,21,22,23,24,25,26,27,28,29,30,
        31,32,33,34,35,36,37,38,39,40,41,43,44,45,46,48,49,50,51,52,53,54,55,56,57,58,
        59,60,61,62,63,64,65,66,67,68,69,70,71,72,73,74,75,76,77,78,79,80,81,82,83,84,
        85,86,87,88,89,90,91,92,93,94,95,96,11001003,22001001,33001001,44001002
    ];

    #[test]
    fn the_initial_stamp_set_matches_official() {
        let ids: Vec<i64> = owned_stamp_ids().members().map(|s| s.as_i64().unwrap()).collect();
        assert_eq!(ids, OFFICIAL_INITIAL_STAMPS.to_vec());
        // Order matters: the official list is masterdata order, not sorted (the 11xxxxxx
        // band trails the small ids).
        assert_eq!(ids.first(), Some(&1));
        assert_eq!(ids.last(), Some(&44001002));
        // The gaps are real - 18, 42 and 47 are not initial stamps.
        for missing in [18, 42, 47] {
            assert!(!ids.contains(&missing), "{missing} should not be an initial stamp");
        }
    }

    #[test]
    fn get_stamp_and_chat_home_cannot_disagree() {
        // Both endpoints read the one source; this is what stops /chat/home's list and
        // the stamp picker's list from drifting apart.
        assert_eq!(owned_stamp_ids(), *databases::INITIAL_CHAT_STAMPS);
        assert!(!owned_stamp_ids().is_empty());
    }
}

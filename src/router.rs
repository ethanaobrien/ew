pub mod start;
pub mod global;
pub mod login;
pub mod userdata;
pub mod user;
pub mod purchase;
pub mod tutorial;
pub mod mission;
pub mod home;
pub mod lottery;
pub mod friend;
pub mod live;
pub mod event;
pub mod chat;
pub mod story;
pub mod notice;
pub mod debug;
pub mod gree;
pub mod serial_code;
pub mod web;
pub mod card;
pub mod shop;
pub mod webui;
pub mod clear_rate;
pub mod exchange;
pub mod items;
pub mod databases;
pub mod location;
pub mod event_ranking;

use actix_web::{
    HttpResponse,
    HttpRequest,
    http::header::HeaderValue,
    http::header::HeaderMap
};
use json::{JsonValue, object};
use crate::encryption;

fn unhandled(req: HttpRequest, body: String) -> Option<JsonValue> {
    if body != String::new() {
        println!("{}", encryption::decrypt_packet(&body).unwrap_or(body));
    }
    println!("Unhandled request: {}", req.path());
    None
}

fn not_found(headers: &HeaderMap) -> HttpResponse {
    let rv = object!{
        "code": 4,
        "server_time": global::timestamp(),
        "message": ""
    };
    return global::send(rv, 0, &headers)
}

async fn api_req(req: HttpRequest, body: String) -> HttpResponse {
    let headers = req.headers().clone();
    let args = crate::get_args();
    if args.hidden && (req.path().starts_with("/api/webui/") || !(req.path().starts_with("/api") || req.path().starts_with("/v1.0"))) {
        return not_found(&headers);
    } else if !req.path().starts_with("/api") && !req.path().starts_with("/v1.0") {
        return webui::main(req);
    }
    if headers.get("a6573cbe").is_none() && req.path().starts_with("/api") {
        if args.hidden {
            return not_found(&headers);
        } else {
            return webui::main(req);
        }
    }
    let blank_header = HeaderValue::from_static("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("").parse::<i64>().unwrap_or(0);
    let resp: Option<JsonValue> = if req.method() == "POST" {
        match req.path() {
            "/api/debug/error" => debug::error(req, body),
            "/api/start" => start::start(req, body),
            "/api/start/assetHash" => start::asset_hash(req, body),
            "/api/dummy/login" => login::dummy(req, body),
            "/api/user" => user::user_post(req, body),
            "/api/chat/home" => chat::home(req, body),
            "/api/chat/talk/start" => chat::start(req, body),
            "/api/chat/talk/end" => chat::end(req, body),
            "/api/story/read" => story::read(req, body),
            "/api/user/initialize" => user::initialize(req, body),
            "/api/user/detail" => user::detail(req, body),
            "/api/gift" => user::gift(req, body),
            "/api/deck" => user::deck(req, body),
            "/api/tutorial" => tutorial::tutorial(req, body),
            "/api/friend" => friend::friend(req, body),
            "/api/friend/search" => friend::search(req, body),
            "/api/friend/search/recommend" => friend::recommend(req, body),
            "/api/friend/request" => friend::request(req, body),
            "/api/friend/request/approve" => friend::approve(req, body),
            "/api/friend/request/cancel" => friend::cancel(req, body),
            "/api/friend/delete" => friend::delete(req, body),
            "/api/live/guest" => live::guest(req, body),
            "/api/live/mission" => live::mission(req, body),
            "/api/live/ranking" => clear_rate::ranking(req, body),
            "/api/event" => event::event(req, body),
            "/api/event/star_event" => event::star_event(req, body),
            "/api/event/set/member" => event::set_member(req, body),
            "/api/event/ranking" => event::ranking(req, body),
            "/api/event_star_live/change_target_music" => event::change_target_music(req, body),
            "/api/event_star_live/start" => live::event_start(req, body),
            "/api/event_star_live/end" => event::event_end(req, body),
            "/api/event_star_live/skip" => event::event_skip(req, body),
            "/api/live/start" => live::start(req, body),
            "/api/live/end" => live::end(req, body),
            "/api/live/skip" => live::skip(req, body),
            "/api/live/retire" => live::retire(req, body),
            "/api/live/continue" => live::continuee(req, body),
            "/api/live/reward" => live::reward(req, body),
            "/api/mission/clear" => mission::clear(req, body),
            "/api/mission/receive" => mission::receive(req, body),
            "/api/home/preset" => home::preset(req, body),
            "/api/lottery/get_tutorial" => lottery::tutorial(req, body),
            "/api/lottery" => lottery::lottery_post(req, body),
            "/api/login_bonus" => login::bonus(req, body),
            "/api/login_bonus/event" => login::bonus_event(req, body),
            "/api/notice/reward" => notice::reward_post(req, body),
            "/api/user/getmigrationcode" => user::get_migration_code(req, body),
            "/api/user/registerpassword" => user::register_password(req, body),
            "/api/user/migration" => user::migration(req, body),
            "/api/user/gglrequestmigrationcode" => user::request_migration_code(req, body),
            "/api/user/gglverifymigrationcode" => user::verify_migration_code(req, body),
            "/api/serial_code" => serial_code::serial_code(req, body),
            "/api/card/reinforce" => card::reinforce(req, body),
            "/api/card/skill/reinforce" => card::skill_reinforce(req, body),
            "/api/card/evolve" => card::evolve(req, body),
            "/api/shop/buy" => shop::buy(req, body),
            "/api/user/getregisteredplatformlist" => user::getregisteredplatformlist(req, body),
            "/api/user/sif/migrate" => user::sif_migrate(req, body).await,
            "/api/user/ss/migrate" => user::sifas_migrate(req, body),
            "/api/exchange" => exchange::exchange_post(req, body),
            "/api/item/use" => items::use_item_req(req, body),
            _ => unhandled(req, body)
        }
    } else {
        match req.path() {
            "/api/user" => user::user(req),
            "/api/gift" => home::gift_get(req),
            "/api/purchase" => purchase::purchase(req),
            "/api/friend/ids" => friend::ids(req),
            "/api/live/clearRate" => clear_rate::clearrate(req),
            "/api/mission" => mission::mission(req),
            "/api/home" => home::home(req),
            "/api/home/preset" => home::preset_get(req),
            "/api/lottery" => lottery::lottery(req),
            "/api/notice/reward" => notice::reward(req),
            "/api/serial_code/events" => serial_code::events(req),
            "/api/album/sif" => user::sif(req),
            "/api/home/announcement" => user::announcement(req),
            "/api/shop" => shop::shop(req),
            "/api/exchange" => exchange::exchange(req),
            "/api/location" => location::location(req),
            _ => unhandled(req, body)
        }
    };
    if resp.is_some() {
        let rv = object!{
            "code": 0,
            "server_time": global::timestamp(),
            "data": resp.unwrap()
        };
        global::send(rv, uid, &headers)
    } else {
        let rv = object!{
            "code": 4,//Idontnermemrmemremremermrme   <-- I think I was not okay when I put this note because I dont remmebr doing it
            "server_time": global::timestamp(),
            "message": ""
        };
        global::send(rv, uid, &headers)
    }
}

pub async fn request(req: HttpRequest, body: String) -> HttpResponse {
    let args = crate::get_args();
    if args.hidden && req.path().starts_with("/api/webui/") {
        return not_found(&req.headers());
    }
    if req.path().starts_with("/v1.0") && req.headers().get("Authorization").is_none() {
        if args.hidden {
            return gree::not_found();
        } else {
            return webui::main(req);
        }
    }
    if req.method() == "POST" {
        match req.path() {
            "/v1.0/auth/initialize" => gree::initialize(req, body),
            "/v1.0/moderate/filtering/commit" => gree::moderate_commit(req, body),
            "/v1.0/auth/authorize" => gree::authorize(req, body),
            "/v1.0/migration/code/verify" => gree::migration_verify(req, body),
            "/v1.0/migration/password/register" => gree::migration_password_register(req, body),
            "/v1.0/migration" => gree::migration(req, body),
            "/api/webui/login" => webui::login(req, body),
            "/api/webui/startLoginbonus" => webui::start_loginbonus(req, body),
            "/api/webui/import" => webui::import(req, body),
            "/api/webui/set_time" => webui::set_time(req, body),
            "/api/webui/admin" => webui::admin_post(req, body),
            _ => api_req(req, body).await
        }
    } else {
        match req.path() {
            "/v1.0/auth/x_uid" => gree::uid(req),
            "/v1.0/payment/productlist" => gree::payment(req),
            "/v1.0/payment/subscription/productlist" => gree::payment(req),
            "/v1.0/payment/ticket/status" => gree::payment_ticket(req),
            "/v1.0/moderate/keywordlist" => gree::moderate_keyword(req),
            "/v1.0/migration/code" => gree::migration_code(req),
            "/v1.0/payment/balance" => gree::balance(req),
            "/web/announcement" => web::announcement(req),
            "/api/webui/userInfo" => webui::user(req),
            "/webui/logout" => webui::logout(req),
            "/api/webui/admin" => webui::admin(req),
            "/api/webui/export" => webui::export(req),
            "/api/webui/serverInfo" => webui::server_info(req),
            _ => api_req(req, body).await
        }
    }
}

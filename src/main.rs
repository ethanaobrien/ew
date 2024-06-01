mod encryption;
mod router;
mod sql;

use json::object;
use actix_web::{
    App,
    HttpServer,
    get,
    HttpResponse,
    HttpRequest,
    web,
    dev::Service,
    http::header::ContentType,
    http::header::HeaderValue
};
use crate::router::global;
use json::JsonValue;

fn unhandled(req: HttpRequest, body: String) -> Option<JsonValue> {
    if body != String::new() {
        println!("{}", encryption::decrypt_packet(&body).unwrap_or(body));
    }
    println!("Unhandled request: {}", req.path());
    None
}

fn api_req(req: HttpRequest, body: String) -> HttpResponse {
    let headers = req.headers().clone();
    if !req.path().starts_with("/api") && !req.path().starts_with("/v1.0") {
        return router::webui::main(req);
    }
    let blank_header = HeaderValue::from_static("");
    let uid = req.headers().get("aoharu-user-id").unwrap_or(&blank_header).to_str().unwrap_or("").parse::<i64>().unwrap_or(0);
    let resp: Option<JsonValue> = if req.method() == "POST" {
        match req.path() {
            "/api/debug/error" => router::debug::error(req, body),
            "/api/start" => router::start::start(req, body),
            "/api/start/assetHash" => router::start::asset_hash(req, body),
            "/api/dummy/login" => router::login::dummy(req, body),
            "/api/user" => router::user::user_post(req, body),
            "/api/chat/home" => router::chat::home(req, body),
            "/api/chat/talk/start" => router::chat::start(req, body),
            "/api/chat/talk/end" => router::chat::end(req, body),
            "/api/story/read" => router::story::read(req, body),
            "/api/user/initialize" => router::user::initialize(req, body),
            "/api/user/detail" => router::user::detail(req, body),
            "/api/gift" => router::user::gift(req, body),
            "/api/deck" => router::user::deck(req, body),
            "/api/tutorial" => router::tutorial::tutorial(req, body),
            "/api/friend" => router::friend::friend(req, body),
            "/api/friend/search" => router::friend::search(req, body),
            "/api/friend/search/recommend" => router::friend::recommend(req, body),
            "/api/friend/request" => router::friend::request(req, body),
            "/api/friend/request/approve" => router::friend::approve(req, body),
            "/api/friend/request/cancel" => router::friend::cancel(req, body),
            "/api/friend/delete" => router::friend::delete(req, body),
            "/api/live/guest" => router::live::guest(req, body),
            "/api/live/mission" => router::live::mission(req, body),
            "/api/live/ranking" => router::clear_rate::ranking(req, body),
            "/api/event" => router::event::event(req, body),
            "/api/event/star_event" => router::event::star_event(req, body),
            "/api/event_star_live/change_target_music" => router::event::change_target_music(req, body),
            "/api/event_star_live/start" => router::live::event_start(req, body),
            "/api/event_star_live/end" => router::live::event_end(req, body),
            //            "/api/event_star_live/skip" => router::live::event_skip(req, body),
            "/api/live/start" => router::live::start(req, body),
            "/api/live/end" => router::live::end(req, body),
            "/api/live/skip" => router::live::skip(req, body),
            "/api/live/retire" => router::live::retire(req, body),
            "/api/live/continue" => router::live::continuee(req, body),
            "/api/live/reward" => router::live::reward(req, body),
            "/api/mission/clear" => router::mission::clear(req, body),
            "/api/mission/receive" => router::mission::receive(req, body),
            "/api/home/preset" => router::home::preset(req, body),
            "/api/lottery/get_tutorial" => router::lottery::tutorial(req, body),
            "/api/lottery" => router::lottery::lottery_post(req, body),
            "/api/login_bonus" => router::login::bonus(req, body),
            "/api/login_bonus/event" => router::login::bonus_event(req, body),
            "/api/notice/reward" => router::notice::reward_post(req, body),
            "/api/user/getmigrationcode" => router::user::get_migration_code(req, body),
            "/api/user/registerpassword" => router::user::register_password(req, body),
            "/api/user/migration" => router::user::migration(req, body),
            "/api/user/gglrequestmigrationcode" => router::user::request_migration_code(req, body),
            "/api/user/gglverifymigrationcode" => router::user::verify_migration_code(req, body),
            "/api/serial_code" => router::serial_code::serial_code(req, body),
            "/api/card/reinforce" => router::card::reinforce(req, body),
            "/api/card/skill/reinforce" => router::card::skill_reinforce(req, body),
            "/api/card/evolve" => router::card::evolve(req, body),
            "/api/shop/buy" => router::shop::buy(req, body),
            "/api/user/getregisteredplatformlist" => router::user::getregisteredplatformlist(req, body),
            "/api/user/sif/migrate" => router::user::sif_migrate(req, body),
            "/api/user/ss/migrate" => router::user::sifas_migrate(req, body),
            "/api/exchange" => router::exchange::exchange_post(req, body),
            "/api/item/use" => router::items::use_item_req(req, body),
            _ => unhandled(req, body)
        }
    } else {
        match req.path() {
            "/api/user" => router::user::user(req),
            "/api/gift" => router::home::gift_get(req),
            "/api/purchase" => router::purchase::purchase(req),
            "/api/friend/ids" => router::friend::ids(req),
            "/api/live/clearRate" => router::clear_rate::clearrate(req),
            "/api/mission" => router::mission::mission(req),
            "/api/home" => router::home::home(req),
            "/api/home/preset" => router::home::preset_get(req),
            "/api/lottery" => router::lottery::lottery(req),
            "/api/notice/reward" => router::notice::reward(req),
            "/api/serial_code/events" => router::serial_code::events(req),
            "/api/album/sif" => router::user::sif(req),
            "/api/home/announcement" => router::user::announcement(req),
            "/api/shop" => router::shop::shop(req),
            "/api/exchange" => router::exchange::exchange(req),
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
            "code": 2,//Idontnermemrmemremremermrme
            "server_time": global::timestamp(),
            "data": ""
        };
        global::send(rv, uid, &headers)
    }
}

async fn request(req: HttpRequest, body: String) -> HttpResponse {
    if req.method() == "POST" {
        match req.path() {
            "/v1.0/auth/initialize" => router::gree::initialize(req, body),
            "/v1.0/moderate/filtering/commit" => router::gree::moderate_commit(req, body),
            "/v1.0/auth/authorize" => router::gree::authorize(req, body),
            "/v1.0/migration/code/verify" => router::gree::migration_verify(req, body),
            "/v1.0/migration/password/register" => router::gree::migration_password_register(req, body),
            "/v1.0/migration" => router::gree::migration(req, body),
            "/api/webui/login" => router::webui::login(req, body),
            "/api/webui/startLoginbonus" => router::webui::start_loginbonus(req, body),
            "/api/webui/import" => router::webui::import(req, body),
            "/api/webui/set_time" => router::webui::set_time(req, body),
            "/api/webui/admin" => router::webui::admin_post(req, body),
            _ => api_req(req, body)
        }
    } else {
        match req.path() {
            "/v1.0/auth/x_uid" => router::gree::uid(req),
            "/v1.0/payment/productlist" => router::gree::payment(req),
            "/v1.0/payment/subscription/productlist" => router::gree::payment(req),
            "/v1.0/payment/ticket/status" => router::gree::payment_ticket(req),
            "/v1.0/moderate/keywordlist" => router::gree::moderate_keyword(req),
            "/v1.0/migration/code" => router::gree::migration_code(req),
            "/v1.0/payment/balance" => router::gree::balance(req),
            "/web/announcement" => router::web::announcement(req),
            "/api/webui/userInfo" => router::webui::user(req),
            "/webui/logout" => router::webui::logout(req),
            "/api/webui/admin" => router::webui::admin(req),
            "/api/webui/export" => router::webui::export(req),
            _ => api_req(req, body)
        }
    }
}

#[get("/index.css")]
async fn css(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::TEXT_CSS))
        .body(include_file!("webui/dist/index.css"))
}
#[get("/index.js")]
async fn js(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::APPLICATION_JAVASCRIPT_UTF_8))
        .body(include_file!("webui/dist/index.js"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let rv = HttpServer::new(|| App::new()
        .wrap_fn(|req, srv| {
            println!("Request: {}", req.path());
            srv.call(req)
        })
        .app_data(web::PayloadConfig::default().limit(1024 * 1024 * 25))
        .service(css)
        .service(js)
        .default_service(web::route().to(request))
    ).bind(("0.0.0.0", 8080))?.run();
    println!("Server started: http://127.0.0.1:{}", 8080);
    rv.await
}



#[macro_export]
macro_rules! include_file {
    ( $s:expr ) => {
        {
            let file = include_flate_codegen::deflate_file!($s);
            let ret = $crate::decode(file);
            std::string::String::from_utf8(ret).unwrap()
        }
    };
}
pub fn decode(bytes: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Read};

    let mut dec = libflate::deflate::Decoder::new(Cursor::new(bytes));
    let mut ret = Vec::new();
    dec.read_to_end(&mut ret).unwrap();
    ret
}

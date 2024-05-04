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
    http::header::ContentType
};

fn unhandled(req: HttpRequest, body: String) -> HttpResponse {
    if !req.path().starts_with("/api") {
        return router::webui::main(req);
    }
    if body != String::new() {
        println!("{}", encryption::decrypt_packet(&body).unwrap_or(body));
    }
    println!("Unhandled request: {}", req.path());
    let resp = object!{
        "code": 2,
        "server_time": router::global::timestamp(),
        "data": ""
    };
    router::global::send(resp)
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
            "/api/live/start" => router::live::start(req, body),
            "/api/live/end" => router::live::end(req, body),
            "/api/live/skip" => router::live::skip(req, body),
            "/api/live/retire" => router::live::retire(req, body),
            "/api/live/continue" => router::live::continuee(req, body),
            "/api/mission/clear" => router::mission::clear(req, body),
            "/api/home/preset" => router::home::preset(req, body),
            "/api/lottery/get_tutorial" => router::lottery::tutorial(req, body),
            "/api/lottery" => router::lottery::lottery_post(req, body),
            "/api/login_bonus" => router::login::bonus(req, body),
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
            "/api/webui/login" => router::webui::login(req, body),
            "/api/webui/startLoginbonus" => router::webui::start_loginbonus(req, body),
            "/api/webui/import" => router::webui::import(req, body),
            "/api/user/getregisteredplatformlist" => router::user::getregisteredplatformlist(req, body),
            "/api/user/sif/migrate" => router::user::sif_migrate(req, body),
            "/api/user/ss/migrate" => router::user::sifas_migrate(req, body),
            "/api/exchange" => router::exchange::exchange_post(req, body),
            _ => unhandled(req, body)
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
            "/api/user" => router::user::user(req),
            "/api/gift" => router::home::gift_get(req),
            "/api/purchase" => router::purchase::purchase(req),
            "/api/friend/ids" => router::friend::ids(req),
            "/api/live/clearRate" => router::clear_rate::clearrate(req),
            "/api/mission" => router::mission::mission(req),
            "/api/mission/receive" => router::mission::receive(req, body),
            "/api/home" => router::home::home(req),
            "/api/home/preset" => router::home::preset_get(req),
            "/api/lottery" => router::lottery::lottery(req),
            "/api/notice/reward" => router::notice::reward(req),
            "/api/serial_code/events" => router::serial_code::events(req),
            "/api/album/sif" => router::user::sif(req),
            "/web/announcement" => router::web::announcement(req),
            "/api/home/announcement" => router::user::announcement(req),
            "/api/shop" => router::shop::shop(req),
            "/api/webui/userInfo" => router::webui::user(req),
            "/webui/logout" => router::webui::logout(req),
            "/api/exchange" => router::exchange::exchange(req),
            _ => unhandled(req, body)
        }
    }
}

#[get("/index.css")]
async fn css(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::TEXT_CSS))
        .body(include_str!("../webui/dist/index.css"))
}
#[get("/index.js")]
async fn js(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::APPLICATION_JAVASCRIPT_UTF_8))
        .body(include_str!("../webui/dist/index.js"))
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    let rv = HttpServer::new(|| App::new()
        .wrap_fn(|req, srv| {
            println!("Request: {}", req.path());
            srv.call(req)
        })
        .service(css)
        .service(js)
        .default_service(web::route().to(request)))
        .bind(("0.0.0.0", 8080))?
        .run();
    println!("Server started: http://127.0.0.1:{}", 8080);
    rv.await
}

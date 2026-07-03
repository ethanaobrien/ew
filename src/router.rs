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
pub mod custom_song;
pub mod webui;
pub mod clear_rate;
pub mod exchange;
pub mod items;
pub mod databases;
pub mod location;
pub mod event_ranking;
pub mod asset_lists;
mod master_data;
mod tools;

use actix_web::{
    HttpResponse,
    HttpRequest,
    body::{BoxBody, MessageBody},
    dev::{ServiceRequest, ServiceResponse},
    middleware::{from_fn, Next},
    http::header::HeaderMap
};
use jzon::{JsonValue, object};
use crate::encryption;

// Requests without client headers (a browser) get the webui
async fn webui_fallback(req: ServiceRequest, next: Next<impl MessageBody + 'static>) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let is_from_game = req.headers().get("aoharu-asset-version").is_some() || req.path().starts_with("/api/webui");
    if !is_from_game {
        let req = req.into_parts().0;
        let resp = if crate::get_args().hidden {
            not_found(req.headers())
        } else {
            webui::main(req.clone())
        };
        return Ok(ServiceResponse::new(req, resp));
    }
    Ok(next.call(req).await?.map_into_boxed_body())
}

fn unhandled(req: &HttpRequest, body: String) -> Option<JsonValue> {
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
    global::send(rv, 0, headers)
}

// Fallback for paths no actix route matched. Game endpoints live in each module's routes()
async fn api_req(req: HttpRequest, body: String) -> HttpResponse {
    let args = crate::get_args();
    if args.hidden && (req.path().starts_with("/api/webui/") || !(req.path().starts_with("/api") || req.path().starts_with("/v1.0"))) {
        return not_found(req.headers());
    } else if !req.path().starts_with("/api") && !req.path().starts_with("/v1.0") {
        return webui::main(req);
    }
    let resp = unhandled(&req, body);
    global::api(&req, resp)
}

pub async fn request(req: HttpRequest, body: String) -> HttpResponse {
    let args = crate::get_args();
    let headers = req.headers();
    if args.hidden && (req.path().starts_with("/api/webui/") || req.path().starts_with("/live_clear_rate.html")) {
        return not_found(headers);
    }
    if headers.get("aoharu-asset-version").is_none() && req.path().starts_with("/api") && !req.path().starts_with("/api/webui") {
        if args.hidden {
            return not_found(headers);
        } else {
            return webui::main(req);
        }
    }
    if req.method() == "POST" {
        match req.path() {
            "/api/webui/login" => webui::login(req, body),
            "/api/webui/startLoginbonus" => webui::start_loginbonus(req, body),
            "/api/webui/import" => webui::import(req, body),
            "/api/webui/set_time" => webui::set_time(req, body),
            "/api/webui/cheat" => webui::cheat(req, body),
            _ => api_req(req, body).await
        }
    } else {
        match req.path() {
            "/web/announcement" => web::announcement(req),
            "/api/webui/userInfo" => webui::user(req),
            "/live_clear_rate.html" => clear_rate::clearrate_html(req).await,
            "/webui/logout" => webui::logout(req),
            "/api/webui/export" => webui::export(req),
            "/api/webui/serverInfo" => webui::server_info(req),
            "/api/webui/listCards" => webui::get_card_info(req),
            "/api/webui/listMusic" => webui::get_music_info(req),
            "/api/webui/listLoginBonus" => webui::list_login_bonus(req),
            "/api/webui/listItems" => webui::list_items(req),
            _ => api_req(req, body).await
        }
    }
}

pub fn configure(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.configure(crate::static_handlers::routes);
    cfg.service(
        actix_web::web::scope("/api")
            .configure(asset_lists::routes)
            .configure(master_data::routes)
            .service(
                actix_web::web::scope("")
                    .wrap(from_fn(webui_fallback))
                    // Split between user (claiming) and home (listing)
                    .service(
                        actix_web::web::resource("/gift")
                            .route(actix_web::web::get().to(home::gift_get))
                            .route(actix_web::web::post().to(user::gift))
                    )
                    .configure(card::routes)
                    .configure(chat::routes)
                    .configure(custom_song::routes)
                    .configure(debug::routes)
                    .configure(event::routes)
                    .configure(exchange::routes)
                    .configure(friend::routes)
                    .configure(home::routes)
                    .configure(items::routes)
                    .configure(live::routes)
                    .configure(location::routes)
                    .configure(login::routes)
                    .configure(lottery::routes)
                    .configure(mission::routes)
                    .configure(notice::routes)
                    .configure(purchase::routes)
                    .configure(serial_code::routes)
                    .configure(shop::routes)
                    .configure(start::routes)
                    .configure(story::routes)
                    .configure(tutorial::routes)
                    .configure(user::routes)
            )
    );
    cfg.service(
        actix_web::web::scope("/v1.0")
            .configure(gree::routes)
    );
    cfg.configure(custom_song::web_routes);
}

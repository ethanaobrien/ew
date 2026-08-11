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
pub mod multi_live;
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
pub mod custom_card;
pub mod rich_text;
pub mod webui;
pub mod clear_rate;
pub mod exchange;
pub mod items;
pub mod databases;
pub mod location;
pub mod event_ranking;
mod tools;

use actix_web::{
    HttpResponse,
    HttpRequest,
    FromRequest,
    Responder,
    body::{BoxBody, MessageBody},
    dev::{Payload, ServiceRequest, ServiceResponse},
    middleware::{from_fn, Next},
};
use futures_util::future::LocalBoxFuture;
use futures_util::FutureExt;
use jzon::{JsonValue, object};
use crate::encryption;

pub struct Body(pub JsonValue);

pub struct Login(pub String);

struct SessionError(HttpRequest);

impl std::fmt::Debug for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "invalid session for uid {}", global::get_uid(self.0.headers()))
    }
}

impl std::fmt::Display for SessionError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{:?}", self)
    }
}

impl actix_web::ResponseError for SessionError {
    fn error_response(&self) -> HttpResponse {
        println!("Rejecting request from uid {}: bad session", global::get_uid(self.0.headers()));
        global::api_error(&self.0, global::RESULT_SESSION)
    }
}

pub struct Session {
    pub key: String,
    pub body: JsonValue,
}

pub trait ApiBody {
    fn into_json(self) -> JsonValue;
}

impl ApiBody for JsonValue {
    fn into_json(self) -> JsonValue {
        self
    }
}

pub struct Api<T = JsonValue>(pub Option<T>);

impl<T: ApiBody> Responder for Api<T> {
    type Body = BoxBody;

    fn respond_to(self, req: &HttpRequest) -> HttpResponse {
        global::api(req, self.0.map(ApiBody::into_json))
    }
}

impl FromRequest for Body {
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let fut = String::from_request(req, payload);
        async move {
            Ok(Body(jzon::parse(&encryption::decrypt_packet(&fut.await?).unwrap()).unwrap()))
        }.boxed_local()
    }
}

impl FromRequest for Login {
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let headers = req.headers().clone();
        let req = req.clone();
        let fut = String::from_request(&req, payload);
        async move {
            let key = global::get_login(&headers, &encryption::decrypt_packet(&fut.await?).unwrap());
            if key.is_empty() {
                return Err(SessionError(req).into());
            }
            Ok(Login(key))
        }.boxed_local()
    }
}

impl FromRequest for Session {
    type Error = actix_web::Error;
    type Future = LocalBoxFuture<'static, Result<Self, Self::Error>>;

    fn from_request(req: &HttpRequest, payload: &mut Payload) -> Self::Future {
        let headers = req.headers().clone();
        let req = req.clone();
        let fut = String::from_request(&req, payload);
        async move {
            let body = encryption::decrypt_packet(&fut.await?).unwrap();
            let key = global::get_login(&headers, &body);
            if key.is_empty() {
                return Err(SessionError(req).into());
            }
            Ok(Session { key, body: jzon::parse(&body).unwrap() })
        }.boxed_local()
    }
}

// Requests without client headers (a browser) get the webui
async fn webui_fallback(req: ServiceRequest, next: Next<impl MessageBody + 'static>) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let is_from_game = req.headers().get("aoharu-asset-version").is_some() || req.path().starts_with("/api/webui");
    if !is_from_game {
        let req = req.into_parts().0;
        let resp = if crate::get_args().hidden {
            not_found(&req)
        } else {
            webui::main(req.clone())
        };
        return Ok(ServiceResponse::new(req, resp));
    }
    Ok(next.call(req).await?.map_into_boxed_body())
}

async fn asset_gate(req: ServiceRequest, next: Next<impl MessageBody + 'static>) -> Result<ServiceResponse<BoxBody>, actix_web::Error> {
    let check_hash = !matches!(req.path(), "/api/start" | "/api/start/assetHash");
    if let Some(code) = global::check_asset_headers(req.headers(), check_hash) {
        let req = req.into_parts().0;
        let resp = global::api_error(&req, code);
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

fn not_found(req: &HttpRequest) -> HttpResponse {
    let rv = object!{
        "code": 4,
        "server_time": global::timestamp(),
        "message": ""
    };
    global::send(rv, 0, req)
}

// Fallback for paths no actix route matched. Game endpoints live in each module's routes()
async fn api_req(req: HttpRequest, body: String) -> HttpResponse {
    let args = crate::get_args();
    if args.hidden && (req.path().starts_with("/api/webui/") || !(req.path().starts_with("/api") || req.path().starts_with("/v1.0"))) {
        return not_found(&req);
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
        return not_found(&req);
    }
    if headers.get("aoharu-asset-version").is_none() && req.path().starts_with("/api") && !req.path().starts_with("/api/webui") {
        if args.hidden {
            return not_found(&req);
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
            "/api/webui/grantPermission" => webui::grant_permission(req, body),
            "/api/webui/revokePermission" => webui::revoke_permission(req, body),
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
            "/api/webui/listPermissions" => webui::list_permissions(req),
            "/api/webui/listCharacters" => webui::list_characters(req),
            "/api/webui/listSkillCenters" => webui::list_skill_centers(req),
            "/api/webui/customCardLimits" => webui::custom_card_limits(req),
            "/api/webui/myScopes" => webui::my_scopes(req),
            _ => api_req(req, body).await
        }
    }
}

pub fn configure(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.configure(crate::static_handlers::routes);
    cfg.service(
        actix_web::web::scope("/api")
            .service(
                actix_web::web::scope("")
                    .wrap(from_fn(webui_fallback))
                    .wrap(from_fn(asset_gate))
                    // Split between user (claiming) and home (listing)
                    .service(
                        actix_web::web::resource("/gift")
                            .route(actix_web::web::get().to(home::gift_get))
                            .route(actix_web::web::post().to(user::gift))
                    )
                    .configure(card::routes)
                    .configure(chat::routes)
                    .configure(custom_song::routes)
                    .configure(custom_card::routes)
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
                    .configure(multi_live::routes)
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
    cfg.configure(custom_card::web_routes);
}

pub mod start;
pub mod arcade;
pub mod global;
pub mod login;
pub mod userdata;
pub mod user;
pub mod purchase;
pub mod tutorial;
pub mod mission;
pub mod beginner_mission;
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
pub mod custom_group;
pub mod custom_3dmv;
pub mod rich_text;
pub mod webui;
pub mod clear_rate;
pub mod exchange;
pub mod items;
pub mod databases;
pub mod location;
pub mod event_ranking;
pub mod errors;
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
use jzon::JsonValue;
use crate::encryption;
use errors::{ErrorResponse, ResultCode};

pub struct Body(pub JsonValue);

pub struct Login(pub String);

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
                return Err(ErrorResponse(req, ResultCode::ErrorSession).into());
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
                return Err(ErrorResponse(req, ResultCode::ErrorSession).into());
            }
            Ok(Session { key, body: jzon::parse(&body).unwrap() })
        }.boxed_local()
    }
}

// Default fallback handler
pub async fn request(req: HttpRequest) -> HttpResponse {
    let args = crate::get_args();
    if req.path().starts_with("/api") || args.hidden {
        return global::api_error(&req, ResultCode::ErrorNotFound);
    }
    webui::main(req)
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

pub fn configure(cfg: &mut actix_web::web::ServiceConfig) {
    cfg.configure(crate::static_handlers::routes);
    cfg.service(
        actix_web::web::scope("/api")
            .service(
                actix_web::web::scope("")
                    .wrap(from_fn(asset_gate))
                    // Split between user (claiming) and home (listing)
                    .service(
                        actix_web::web::resource("/gift")
                            .route(actix_web::web::get().to(home::gift_get))
                            .route(actix_web::web::post().to(user::gift))
                    )
                    .configure(arcade::routes)
                    .configure(card::routes)
                    .configure(chat::routes)
                    .configure(custom_song::routes)
                    .configure(custom_card::routes)
                    .configure(custom_3dmv::routes)
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
                    .configure(webui::api_routes)
            )
    );
    cfg.service(
        actix_web::web::scope("/v1.0")
            .configure(gree::routes)
    );
    cfg.configure(custom_song::web_routes);
    cfg.configure(custom_card::web_routes);
    cfg.configure(custom_group::web_routes);
    cfg.configure(custom_3dmv::web_routes);
    cfg.configure(web::routes);
    cfg.configure(clear_rate::routes);
    cfg.configure(webui::routes);
}

use actix_web::{HttpRequest, web, Responder, HttpResponse};
use actix_web::http::header::ContentType;
use jzon::object;
use crate::include_file;

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/assetLists")
            .route("/supported", web::get().to(supported))
            .route("{platform}/{LANG}", web::get().to(get))
    );
}

async fn get(_req: HttpRequest) -> impl Responder {
    let mut response = object!{};
    response["Bundle"] = include_file!("src/router/asset_lists/Bundle.json").into();
    response["Movie"] = include_file!("src/router/asset_lists/Movie.json").into();
    response["Sound"] = include_file!("src/router/asset_lists/Sound.json").into();

    let body = jzon::stringify(response);
    HttpResponse::Ok()
        .insert_header(("content-type", ContentType::json()))
        .insert_header(("content-length", body.len()))
        .body(body)
}

async fn supported() -> impl Responder {
    "SUPPORTED"
}

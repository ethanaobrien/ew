use actix_web::{web, HttpRequest, HttpResponse, Responder};
use actix_web::http::header::ContentType;
use crate::router::databases::csv::{get_all, Region};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/masterdata")
            .route("/supported", web::get().to(supported))
            .route("/{platform}/{LANG}", web::get().to(mst))
    );
}

async fn mst(req: HttpRequest) -> impl Responder {
    let lang = req.match_info().get("LANG").unwrap_or("JP");

    let region = match lang.to_ascii_uppercase().as_str() {
        "JP" => Region::Jp,
        _    => Region::En, // idk
    };

    let body = get_all(region);
    let body = jzon::stringify(body);
    HttpResponse::Ok()
        .insert_header(("content-type", ContentType::json()))
        .insert_header(("content-length", body.len()))
        .body(body)
}

async fn supported() -> impl Responder {
    "SUPPORTED"
}

use actix_web::{web, HttpRequest, HttpResponse, Responder};

use crate::router::databases::csv::{self, Region};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/masterdata/{platform}/{LANG}")
            .route("/{MST}", web::get().to(mst))
    );
}

async fn mst(req: HttpRequest) -> impl Responder {
    let mst_name = req.match_info().get("MST").unwrap();
    let lang = req.match_info().get("LANG").unwrap_or("JP");

    let region = match lang.to_ascii_uppercase().as_str() {
        "JP" => Region::Jp,
        _    => Region::En, // idk
    };

    match csv::csv_bytes(region, mst_name) {
        Some(body) => HttpResponse::Ok()
            .insert_header(("content-type", "text/csv; charset=utf-8"))
            .insert_header(("content-length", body.len()))
            .body(body),
        None => HttpResponse::NotFound().finish(),
    }
}

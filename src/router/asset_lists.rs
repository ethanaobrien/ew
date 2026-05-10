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
    response["Bundle"] = load_list("Bundle").into();
    response["Movie"] = load_list("Movie").into();
    response["Sound"] = load_list("Sound").into();

    let body = jzon::stringify(response);
    HttpResponse::Ok()
        .insert_header(("content-type", ContentType::json()))
        .insert_header(("content-length", body.len()))
        .body(body)
}

fn load_list(name: &str) -> String {
    let rel = format!("asset_lists/{}.json", name);
    if let Some(bytes) = crate::runtime::read_masterdata_file(&rel) {
        if let Ok(s) = String::from_utf8(bytes) {
            return s;
        }
    }
    match name {
        "Bundle" => include_file!("src/router/asset_lists/Bundle.json"),
        "Movie" => include_file!("src/router/asset_lists/Movie.json"),
        "Sound" => include_file!("src/router/asset_lists/Sound.json"),
        _ => unreachable!(),
    }
}

async fn supported() -> impl Responder {
    "SUPPORTED"
}

use actix_web::{HttpRequest, web, Responder, HttpResponse};
use actix_web::http::header::ContentType;
use jzon::object;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::include_file;

lazy_static! {
    static ref LIST_CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}

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
    if let Some(cached) = LIST_CACHE.lock().unwrap().get(name) {
        return cached.clone();
    }
    let rel = format!("asset_lists/{}.json", name);
    let list = crate::runtime::read_masterdata_file(&rel)
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| match name {
            "Bundle" => include_file!("src/router/asset_lists/Bundle.json"),
            "Movie"  => include_file!("src/router/asset_lists/Movie.json"),
            "Sound"  => include_file!("src/router/asset_lists/Sound.json"),
            _ => unreachable!(),
        });

    LIST_CACHE.lock().unwrap().insert(name.to_string(), list.clone());
    list
}

async fn supported() -> impl Responder {
    "SUPPORTED"
}

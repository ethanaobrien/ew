use actix_web::{
    get,
    HttpResponse,
    HttpRequest,
    http::header::ContentType
};
use crate::include_file;

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
#[get("/maintenance/maintenance.json")]
async fn maintenance(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::APPLICATION_JSON))
        .body(r#"{"opened_at":"2024-02-05 02:00:00","closed_at":"2024-02-05 04:00:00","message":":(","server":1,"gamelib":0}"#)
}
fn handle_assets(req: HttpRequest) -> HttpResponse {
    HttpResponse::SeeOther()
        .insert_header(("location", format!("https://sif2.sif.moe{}", req.path())))
        .body("")
}
#[get("/Android/{hash}/{file}")]
async fn files_jp(req: HttpRequest) -> HttpResponse {
    handle_assets(req)
}

#[get("/Android/{lang}/{hash}/{file}")]
async fn files_gl(req: HttpRequest) -> HttpResponse {
    handle_assets(req)
}

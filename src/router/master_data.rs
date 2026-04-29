use actix_web::{HttpResponse, HttpRequest, web, Responder};
use actix_web::http::header::ContentType;
use include_dir::{include_dir, Dir};

static MASTERDATA: Dir<'_> = include_dir!("src/router/masterdata/json/");

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/masterdata/{platform}/{LANG}")
            .route("/{MST}", web::get().to(mst))
    );
}

async fn mst(req: HttpRequest) -> impl Responder {
    let mst = req.match_info().get("MST").unwrap();
    println!("Getting masterdata {}", mst);
    if let Some(file) = MASTERDATA.get_file(format!("{mst}.json")) {
        let body = file.contents();
        return HttpResponse::Ok()
            .insert_header(ContentType::json())
            .insert_header(("content-length", body.len()))
            .body(body);
    }
    HttpResponse::NotFound().finish()
}

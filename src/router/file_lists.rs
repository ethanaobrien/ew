use actix_web::{HttpResponse, HttpRequest, web, Responder};

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/fileLists/{platform}/{LANG}")
            .route("/Bundle", web::get().to(bundle))
            .route("/Movie", web::get().to(movie))
            .route("/Sound", web::get().to(sound))
    );
}

async fn bundle(_req: HttpRequest) -> impl Responder {
    HttpResponse::Ok().body(include_str!("file_lists/Bundle.json"))
}

async fn movie(_req: HttpRequest) -> impl Responder {
    HttpResponse::Ok().body(include_str!("file_lists/Movie.json"))
}

async fn sound(_req: HttpRequest) -> impl Responder {
    HttpResponse::Ok().body(include_str!("file_lists/Sound.json"))
}

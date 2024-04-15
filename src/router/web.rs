use actix_web::{HttpResponse, HttpRequest};

pub fn announcement(_req: HttpRequest) -> HttpResponse {
    
    HttpResponse::Ok().body("sif2 is back!")
}

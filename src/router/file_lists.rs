use actix_web::{HttpResponse, HttpRequest};

pub fn bundle(_req: HttpRequest) -> HttpResponse {
    
    HttpResponse::Ok().body(include_str!("file_lists/Bundle.json"))
}

use actix_web::{HttpResponse, HttpRequest, http::header::HeaderValue, http::header::ContentType};
use json::object;
use crate::router::userdata;
use crate::router::global;

fn get_login_token(req: &HttpRequest) -> Option<String> {
    let blank_header = HeaderValue::from_static("");
    let cookies = req.headers().get("Cookie").unwrap_or(&blank_header).to_str().unwrap_or("");
    if cookies == "" {
        return None;
    }
    return Some(cookies.split("ew_token=").last().unwrap_or("").split(';').collect::<Vec<_>>()[0].to_string());
}

fn error(msg: &str) -> HttpResponse {
    let resp = object!{
        result: "ERR",
        message: msg
    };
    HttpResponse::Ok()
        .insert_header(("Access-Control-Allow-Origin", "*"))
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
    
}

pub fn login(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&body).unwrap();
    let token = userdata::webui_login(body["uid"].as_i64().unwrap(), &body["password"].to_string());
    
    if token.is_err() {
        return error(&token.unwrap_err());
    }
    
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Set-Cookie", format!("ew_token={}; SameSite=Strict; HttpOnly", token.unwrap())))
        .body(json::stringify(resp))
}

pub fn import(_req: HttpRequest, body: String) -> HttpResponse {
    let body = json::parse(&body).unwrap();
    
    let result = userdata::webui_import_user(body);
    
    if result.is_err() {
        return error(&result.unwrap_err());
    }
    let result = result.unwrap();
    
    let resp = object!{
        result: "OK",
        uid: result["uid"].clone(),
        migration_token: result["migration_token"].clone()
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
}

pub fn user(req: HttpRequest) -> HttpResponse {
    let token = get_login_token(&req);
    if token.is_none() {
        return error("Not logged in");
    }
    let data = userdata::webui_get_user(&token.unwrap());
    if data.is_none() {
        return error("Expired login");
    }
    let mut data = data.unwrap();
    
    data["userdata"]["user"]["rank"] = global::get_user_rank_data(data["userdata"]["user"]["exp"].as_i64().unwrap())["rank"].clone();
    
    let resp = object!{
        result: "OK",
        data: data
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
}

pub fn logout(req: HttpRequest) -> HttpResponse {
    let token = get_login_token(&req);
    if !token.is_none() {
        userdata::webui_logout(&token.unwrap());
    }
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Found()
        .insert_header(ContentType::json())
        .insert_header(("Set-Cookie", "ew_token=deleted; expires=Thu, 01 Jan 1970 00:00:00 GMT"))
        .insert_header(("Location", "/"))
        .body(json::stringify(resp))
}

pub fn main(req: HttpRequest) -> HttpResponse {
    if req.path() == "/" {
        let token = get_login_token(&req);
        if !token.is_none() {
            let data = userdata::webui_get_user(&token.unwrap());
            if !data.is_none() {
                return HttpResponse::Found()
                    .insert_header(("Location", "/home/"))
                    .body("");
            }
        }
    }
    if req.path() != "/" && req.path() != "/home/" && req.path() != "/import/" {
        return HttpResponse::Found()
            .insert_header(("Location", "/"))
            .body("");
    }
    HttpResponse::Ok()
        .insert_header(ContentType::html())
        .body(include_str!("../../webui/dist/index.html"))
}

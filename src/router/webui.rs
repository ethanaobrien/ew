use actix_web::{
    HttpResponse,
    HttpRequest,
    http::header::HeaderValue,
    http::header::ContentType
};
use json::{JsonValue, object};
use std::fs;
use std::fs::File;
use std::io::Write;

use crate::include_file;
use crate::router::{userdata, items};

fn get_config() -> JsonValue {
    let contents = fs::read_to_string("config.json").unwrap_or(String::from("aaaaaaaaaaaaaaaaa"));
    json::parse(&contents).unwrap_or(object!{
        import: true
    })
}
fn save_config(val: String) {
    let mut current = get_config();
    let new = json::parse(&val).unwrap();
    for vall in new.entries() {
        current[vall.0] = vall.1.clone();
    }
    let mut f = File::create("config.json").unwrap();
    f.write_all(json::stringify(current).as_bytes()).unwrap();
}

fn get_login_token(req: &HttpRequest) -> Option<String> {
    let blank_header = HeaderValue::from_static("");
    let cookies = req.headers().get("Cookie").unwrap_or(&blank_header).to_str().unwrap_or("");
    if cookies.is_empty() {
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
    if get_config()["import"].as_bool().unwrap() == false {
        let resp = object!{
            result: "Err",
            message: "Importing accounts is disabled on this server."
        };
        return HttpResponse::Ok()
            .insert_header(ContentType::json())
            .body(json::stringify(resp));
    }
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
    
    data["userdata"]["user"]["rank"] = items::get_user_rank_data(data["userdata"]["user"]["exp"].as_i64().unwrap())["rank"].clone();
    
    let resp = object!{
        result: "OK",
        data: data
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
}

pub fn start_loginbonus(req: HttpRequest, body: String) -> HttpResponse {
    let token = get_login_token(&req);
    if token.is_none() {
        return error("Not logged in");
    }
    let body = json::parse(&body).unwrap();
    let resp = userdata::webui_start_loginbonus(body["bonus_id"].as_i64().unwrap(), &token.unwrap());
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
}

pub fn set_time(req: HttpRequest, body: String) -> HttpResponse {
    let token = get_login_token(&req);
    if token.is_none() {
        return error("Not logged in");
    }
    let body = json::parse(&body).unwrap();
    let resp = userdata::set_server_time(body["timestamp"].as_i64().unwrap(), &token.unwrap());
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
}

pub fn logout(req: HttpRequest) -> HttpResponse {
    let token = get_login_token(&req);
    if token.is_some() {
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
        if token.is_some() {
            let data = userdata::webui_get_user(&token.unwrap());
            if data.is_some() {
                return HttpResponse::Found()
                    .insert_header(("Location", "/home/"))
                    .body("");
            }
        }
    }
    if req.path() != "/" && req.path() != "/home/" && req.path() != "/import/" && req.path() != "/admin/" {
        return HttpResponse::Found()
            .insert_header(("Location", "/"))
            .body("");
    }
    HttpResponse::Ok()
        .insert_header(ContentType::html())
        .body(include_file!("webui/dist/index.html"))
}


macro_rules! check_admin {
    ( $s:expr ) => {
        {
            if $s.peer_addr().unwrap().ip().to_string() != "127.0.0.1" {
                let resp = object!{
                    result: "ERR",
                    message: "Must be on localhost address to access admin panel."
                };
                return HttpResponse::Ok()
                    .insert_header(ContentType::json())
                    .body(json::stringify(resp))
            }
        }
    };
}

pub fn admin(req: HttpRequest) -> HttpResponse {
    check_admin!(req);
    let resp = object!{
        result: "OK",
        data: get_config()
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
}

pub fn admin_post(req: HttpRequest, body: String) -> HttpResponse {
    check_admin!(req);
    save_config(body);
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Ok()
    .insert_header(ContentType::json())
    .body(json::stringify(resp))
}

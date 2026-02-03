use actix_web::{
    HttpResponse,
    HttpRequest,
    http::header::HeaderValue,
    http::header::ContentType
};
use json::{JsonValue, object};

use crate::include_file;
use crate::router::{userdata, items};

//pub const FRONTEND_DOMAIN: &str = "https://sif2-api.ethanthesleepy.one";

fn get_config() -> JsonValue {
    let args = crate::get_args();
    object!{
        import: !args.disable_imports,
        export: !args.disable_exports
    }
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
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
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
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        //.insert_header(("Access-Control-Allow-Credentials", "true"))
        .insert_header(("Set-Cookie", format!("ew_token={}; SameSite=Strict; HttpOnly", token.unwrap())))
        .body(json::stringify(resp))
}

pub fn import(_req: HttpRequest, body: String) -> HttpResponse {
    if !get_config()["import"].as_bool().unwrap() {
        return error("Importing accounts is disabled on this server.");
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
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
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
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        //.insert_header(("Access-Control-Allow-Credentials", "true"))
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
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        //.insert_header(("Access-Control-Allow-Credentials", "true"))
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
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        //.insert_header(("Access-Control-Allow-Credentials", "true"))
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
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        //.insert_header(("Access-Control-Allow-Credentials", "true"))
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
    if req.path() != "/" && req.path() != "/home/" && req.path() != "/import/" && req.path() != "/help/" {
        return HttpResponse::Found()
            .insert_header(("Location", "/"))
            .body("");
    }
    HttpResponse::Ok()
        .insert_header(ContentType::html())
        .body(include_file!("webui/dist/index.html"))
}

pub fn export(req: HttpRequest) -> HttpResponse {
    if !get_config()["export"].as_bool().unwrap() {
        return error("Exporting accounts is disabled on this server.");
    }
    let token = get_login_token(&req);
    if token.is_none() {
        return error("Not logged in");
    }
    let resp = object!{
        result: "OK",
        data: userdata::export_user(&token.unwrap()).unwrap()
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(json::stringify(resp))
}

pub fn server_info(_req: HttpRequest) -> HttpResponse {
    let args = crate::get_args();

    let resp = object!{
        result: "OK",
        data: {
            account_import: get_config()["import"].as_bool().unwrap(),
            links: {
                global: args.global_android,
                japan: args.japan_android,
                ios: {
                    global: args.global_ios,
                    japan: args.japan_ios
                },
                assets: args.assets_url
            }
        }
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        .body(json::stringify(resp))
}

fn get_query_str(req: &HttpRequest, key: &str, def: &str) -> String {
    let query_str = req.query_string();
    query_str
        .split('&')
        .find(|s| s.starts_with(&format!("{key}=")))
        .and_then(|s| s.split('=').nth(1))
        .unwrap_or(def).to_string()
}

pub fn get_card_info(req: HttpRequest) -> HttpResponse {
    let page = get_query_str(&req, "page", "1").parse::<usize>().unwrap_or(1) - 1;
    let max = get_query_str(&req, "max", "10").parse::<usize>().unwrap_or(10);
    let all = get_query_str(&req, "all", "false");
    let name_query = get_query_str(&req, "query", "");

    let start = page * max;

    let items = json::parse(&include_file!("src/router/webui/cards.json")).unwrap();

    if all == "true" {
        let resp = object!{
            total_pages: 1,
            current: items
        };

        return HttpResponse::Ok()
            .content_type(ContentType::json())
            //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
            .body(json::stringify(resp));
    }

    let mut filtered_items: Vec<_> = items.members().collect();

    if !name_query.is_empty() {
        let lowercase_query = name_query.to_lowercase();
        filtered_items.retain(|item| {
            item["name"].to_string().to_lowercase().contains(&lowercase_query)
        });
    }
    
    let total_len = filtered_items.len();

    let page_items: Vec<_> = filtered_items
        .into_iter()
        .skip(start)
        .take(max)
        .cloned()
        .collect();

    if page_items.is_empty() {
        return HttpResponse::NotFound()
            //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
            .finish();
    }

    let total_pages = (total_len as f64 / max as f64).ceil() as usize;

    let resp = object!{
        total_pages: total_pages,
        current: page_items
    };

    HttpResponse::Ok()
        .content_type(ContentType::json())
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        .body(json::stringify(resp))
}

pub fn get_music_info(req: HttpRequest) -> HttpResponse {
    let page = get_query_str(&req, "page", "1").parse::<usize>().unwrap_or(1) - 1;
    let max = get_query_str(&req, "max", "10").parse::<usize>().unwrap_or(10);
    let lang = get_query_str(&req, "lang", "JP");

    let start = page * max;

    let items = if lang == "EN" {
        json::parse(&include_file!("src/router/databases/json/global/music.json")).unwrap()
    } else {
        json::parse(&include_file!("src/router/databases/json/music.json")).unwrap()
    };

    let page_items: Vec<_> = items.members()
        .skip(start)
        .take(max)
        .cloned()
        .collect();

    if page_items.is_empty() {
        return HttpResponse::NotFound()
            //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
            .finish();
    }

    let total_items = items.len();
    let total_pages = (total_items as f64 / max as f64).ceil() as usize;

    let resp = object!{
        total_pages: total_pages,
        current: page_items
    };

    HttpResponse::Ok()
        .content_type(ContentType::json())
        //.insert_header(("Access-Control-Allow-Origin", FRONTEND_DOMAIN))
        .body(json::stringify(resp))
}

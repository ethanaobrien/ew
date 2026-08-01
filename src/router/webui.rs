use actix_web::{
    HttpResponse,
    HttpRequest,
    http::header::HeaderValue,
    http::header::ContentType
};
use jzon::{array, JsonValue, object};
use lazy_static::lazy_static;
use include_dir::{include_dir, Dir};
use std::fs;

use crate::include_file;
use crate::database::permissions;
use crate::router::{userdata, items};
use crate::router::databases::csv::Region;

fn get_config() -> JsonValue {
    let args = crate::get_args();
    object!{
        import: !args.disable_imports,
        export: !args.disable_exports
    }
}

pub fn get_login_token(req: &HttpRequest) -> Option<String> {
    let blank_header = HeaderValue::from_static("");
    let cookies = req.headers().get("Cookie").unwrap_or(&blank_header).to_str().unwrap_or("");
    if cookies.is_empty() {
        return None;
    }
    Some(cookies.split("ew_token=").last().unwrap_or("").split(';').collect::<Vec<_>>()[0].to_string())
}

fn session_uid(req: &HttpRequest) -> Option<i64> {
    let token = get_login_token(req)?;
    let login_token = userdata::webui_login_token(&token)?;
    userdata::get_acc(&login_token)["user"]["id"].as_i64()
}

pub fn error(msg: &str) -> HttpResponse {
    let resp = object!{
        result: "ERR",
        message: msg
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

pub fn login(_req: HttpRequest, body: String) -> HttpResponse {
    let body = jzon::parse(&body).unwrap();
    let token = userdata::webui_login(body["uid"].as_i64().unwrap(), &body["password"].to_string());
    
    if token.is_err() {
        return error(&token.unwrap_err());
    }
    
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .insert_header(("Set-Cookie", format!("ew_token={}; SameSite=Strict; Path=/; HttpOnly", token.unwrap())))
        .body(jzon::stringify(resp))
}

pub fn import(_req: HttpRequest, body: String) -> HttpResponse {
    if !get_config()["import"].as_bool().unwrap() {
        return error("Importing accounts is disabled on this server.");
    }
    let body = jzon::parse(&body).unwrap();
    
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
        .body(jzon::stringify(resp))
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
        .body(jzon::stringify(resp))
}

pub fn start_loginbonus(req: HttpRequest, body: String) -> HttpResponse {
    let token = get_login_token(&req);
    if token.is_none() {
        return error("Not logged in");
    }
    let body = jzon::parse(&body).unwrap();
    let resp = userdata::webui_start_loginbonus(body["bonus_id"].as_i64().unwrap(), &token.unwrap());
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

pub fn set_time(req: HttpRequest, body: String) -> HttpResponse {
    let token = get_login_token(&req);
    if token.is_none() {
        return error("Not logged in");
    }
    let body = jzon::parse(&body).unwrap();
    let resp = userdata::set_server_time(body["timestamp"].as_i64().unwrap(), &token.unwrap());
    
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
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
        .insert_header(("Location", "/login.html"))
        .body(jzon::stringify(resp))
}

static WEBUI_ASSETS: Dir<'_> = include_dir!("webui/");

pub fn main(req: HttpRequest) -> HttpResponse {
    let path = if req.path().ends_with("/") { format!("{}index.html", req.path()) } else { req.path().to_string() };
    let mut chars = path.chars();
    chars.next();
    let path = chars.as_str();

    if path == "login.html" {
        let token = get_login_token(&req);
        if token.is_some() {
            let data = userdata::webui_get_user(&token.unwrap());
            if data.is_some() {
                return HttpResponse::Found()
                    .insert_header(("Location", "/account.html"))
                    .body("");
            }
        }
    }

    if let Some(file) = WEBUI_ASSETS.get_file(&path) {
        let body = file.contents();
        let mime = mime_guess::from_path(path).first_or_octet_stream();
        return HttpResponse::Ok()
            .insert_header(ContentType(mime))
            .insert_header(("content-length", body.len()))
            .body(body);
    } else if path.starts_with("webui/images/card-thumbnails") {
        let args = crate::get_args();

        let file_name = path.split("/").last().unwrap_or("");
        let file_path = format!("{}/{}", args.image_asset_path, file_name).replace("//", "/");
        return if args.image_asset_path != "" && let Ok(body) = fs::read(file_path) {
            let mime = mime_guess::from_path(path).first_or_octet_stream();
            HttpResponse::Ok()
                .insert_header(ContentType(mime))
                .insert_header(("content-length", body.len()))
                .body(body)
        } else {
            if args.image_asset_path != "" {
                println!("File '{file_name}' was requested, but no file was found on the disk!");
            }
            HttpResponse::SeeOther()
                .insert_header(("location", format!("https://sif2-api.ethanthesleepy.one{}", req.path())))
                .body("")
        }
    }

    HttpResponse::Found()
        .insert_header(("Location", "/"))
        .body("")
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
        .body(jzon::stringify(resp))
}

pub fn server_info(_req: HttpRequest) -> HttpResponse {
    let args = crate::get_args();

    let resp = object!{
        result: "OK",
        data: {
            account_import: get_config()["import"].as_bool().unwrap(),
            custom_songs: !crate::router::custom_song::disabled(),
            custom_cards: !crate::router::custom_card::disabled(),
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
        .body(jzon::stringify(resp))
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

    let items = crate::router::databases::csv::table(Region::Jp, "card");

    if all == "true" {
        let resp = object!{
            total_pages: 1,
            current: items
        };

        return HttpResponse::Ok()
            .content_type(ContentType::json())
            .body(jzon::stringify(resp));
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
            .finish();
    }

    let total_pages = (total_len as f64 / max as f64).ceil() as usize;
    let args = crate::get_args();

    let resp = object!{
        total_pages: total_pages,
        current: page_items,
        image_path: args.image_asset_path
    };

    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(jzon::stringify(resp))
}

pub fn get_music_info(req: HttpRequest) -> HttpResponse {
    let page = get_query_str(&req, "page", "1").parse::<usize>().unwrap_or(1) - 1;
    let max = get_query_str(&req, "max", "10").parse::<usize>().unwrap_or(10);
    let lang = get_query_str(&req, "lang", "JP");

    let start = page * max;

    let items = if lang == "EN" {
        crate::router::databases::csv::table(Region::En, "music")
    } else {
        crate::router::databases::csv::table(Region::Jp, "music")
    };

    let page_items: Vec<_> = items.members()
        .skip(start)
        .take(max)
        .cloned()
        .collect();

    if page_items.is_empty() {
        return HttpResponse::NotFound()
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
        .body(jzon::stringify(resp))
}

lazy_static! {
    static ref ITEM: JsonValue = jzon::parse(&include_file!("src/router/webui/item.json")).unwrap();
    static ref LOGIN_BONUS: JsonValue = jzon::parse(&include_file!("src/router/webui/login_bonus.json")).unwrap();
}

pub fn list_login_bonus(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(jzon::stringify(LOGIN_BONUS.clone()))
}

pub fn list_items(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .content_type(ContentType::json())
        .body(jzon::stringify(ITEM.clone()))
}

lazy_static! {
    // The selectable character list for the custom-card form: every official
    // and SIF1-imported character in the baked csv, by name. The import band
    // starts at 5001 (5001-5172 + 6001-6009); official ids top out at 4014
    static ref CHARACTER_CHOICES: JsonValue = {
        let mut rv = jzon::array![];
        for row in crate::router::databases::csv::table(Region::Jp, "character").members() {
            let Some(id) = row["id"].as_i64() else { continue; };
            rv.push(object!{
                id: id,
                name: row["name"].clone(),
                name_en: row["nameEn"].clone(),
                category: if id >= 5000 { "imported" } else { "official" }
            }).unwrap();
        }
        rv
    };

    // The skill_center table with its display strings, for picking a center
    // skill by name instead of by raw id
    static ref SKILL_CENTER_CHOICES: JsonValue = {
        let mut en_rows = object!{};
        for row in crate::router::databases::csv::table(Region::En, "skill_center").members() {
            en_rows[row["id"].to_string()] = row.clone();
        }
        let mut rv = jzon::array![];
        for row in crate::router::databases::csv::table(Region::Jp, "skill_center").members() {
            let en = &en_rows[row["id"].to_string()];
            rv.push(object!{
                id: row["id"].clone(),
                name: row["name"].clone(),
                name_en: en["name"].clone(),
                detail_text: row["detailText"].clone(),
                detail_text_en: en["detailText"].clone()
            }).unwrap();
        }
        rv
    };
}

// The characters a card upload may reference, for the webui's searchable
// picker: the baked official + imported list, plus the custom characters
// this session may build on (their own and the publicly visible ones)
pub fn list_characters(req: HttpRequest) -> HttpResponse {
    let Some(uid) = session_uid(&req) else {
        return error("Not logged in");
    };
    let mut characters = CHARACTER_CHOICES.clone();
    if !crate::router::custom_card::disabled() {
        for character in crate::database::custom_card::get_selectable_characters(uid).members() {
            characters.push(object!{
                id: character["master_character_id"].clone(),
                name: character["name"].clone(),
                name_en: character["name_en"].clone(),
                category: "custom"
            }).unwrap();
        }
    }
    let resp = object!{
        result: "OK",
        characters: characters
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

pub fn list_skill_centers(req: HttpRequest) -> HttpResponse {
    if session_uid(&req).is_none() {
        return error("Not logged in");
    }
    let resp = object!{
        result: "OK",
        skill_centers: SKILL_CENTER_CHOICES.clone()
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

// The concrete upload bounds (per-rarity stat caps, enum ranges, skill array
// lengths) so the form enforces them before submitting
pub fn custom_card_limits(req: HttpRequest) -> HttpResponse {
    if session_uid(&req).is_none() {
        return error("Not logged in");
    }
    let resp = object!{
        result: "OK",
        data: crate::router::custom_card::upload_limits()
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

// The requesting user's own effective scopes, for webui nav gating. Any
// session may ask - it only ever reveals what the user themselves holds
pub fn my_scopes(req: HttpRequest) -> HttpResponse {
    let Some(uid) = session_uid(&req) else {
        return error("Not logged in");
    };
    let resp = object!{
        result: "OK",
        data: {
            uid: uid,
            scopes: permissions::scopes_for(uid),
            can_upload_cards: permissions::has(uid, permissions::CARD_UPLOAD),
            can_publish_cards: permissions::has(uid, permissions::CARD_PUBLISH),
            can_edit_any_cards: permissions::has(uid, permissions::CARD_EDIT),
            can_manage_permissions: permissions::has(uid, permissions::PERMISSION_GRANT)
                || permissions::has(uid, permissions::PERMISSION_REVOKE)
        }
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

// The admin view: every grant plus the grantable vocabulary. Needs a
// permission.* scope - my_scopes is the anyone-can-ask endpoint
pub fn list_permissions(req: HttpRequest) -> HttpResponse {
    let Some(uid) = session_uid(&req) else {
        return error("Not logged in");
    };
    let can_grant = permissions::has(uid, permissions::PERMISSION_GRANT);
    let can_revoke = permissions::has(uid, permissions::PERMISSION_REVOKE);
    if !can_grant && !can_revoke {
        return error("You do not have permission to manage scopes");
    }
    let mut available = array![];
    for scope in permissions::SCOPES {
        available.push(*scope).unwrap();
    }
    let resp = object!{
        result: "OK",
        data: {
            uid: uid,
            can_grant: can_grant,
            can_revoke: can_revoke,
            scopes: permissions::scopes_for(uid),
            available: available,
            grants: permissions::grants()
        }
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

pub fn grant_permission(req: HttpRequest, body: String) -> HttpResponse {
    let Some(uid) = session_uid(&req) else {
        return error("Not logged in");
    };
    let body = jzon::parse(&body).unwrap_or(object!{});
    let target = body["uid"].as_i64().unwrap_or(0);
    let scope = body["scope"].to_string();
    if userdata::get_login_token(target) == String::new() {
        return error(&format!("User {} does not exist", target));
    }
    if let Err(e) = permissions::grant(target, &scope, uid) {
        return error(&e);
    }
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

pub fn revoke_permission(req: HttpRequest, body: String) -> HttpResponse {
    let Some(uid) = session_uid(&req) else {
        return error("Not logged in");
    };
    let body = jzon::parse(&body).unwrap_or(object!{});
    if let Err(e) = permissions::revoke(body["uid"].as_i64().unwrap_or(0), &body["scope"].to_string(), uid) {
        return error(&e);
    }
    let resp = object!{
        result: "OK"
    };
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

pub fn cheat(req: HttpRequest, _body: String) -> HttpResponse {
    let token = get_login_token(&req);
    if token.is_none() {
        return error("Not logged in");
    }
    let key = userdata::webui_login_token(&token.unwrap());
    if key.is_none() {
        return error("Not logged in");
    }
    let key = key.unwrap();
    let mut user = userdata::get_acc_home(&key);

    for item in ITEM.entries() {
        let id = item.0.parse::<i32>().unwrap_or(0);
        let data = item.1;
        if id == 0 {
            continue;
        }
        let reward_type = data["reward_type"].as_i32().unwrap();
        let limit = if reward_type == 4 {
            items::LIMIT_COINS
        } else if reward_type == 1 {
            items::LIMIT_PRIMOGEMS
        } else {
            items::LIMIT_ITEMS
        };
        items::gift_item_basic(id, limit, reward_type, "You have cheated. Here are \"gifts\".", &mut user);
    }

    userdata::save_acc_home(&key, user);

    let resp = object!{
        result: "OK"
    };

    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

#[cfg(test)]
mod tests {
    use super::*;

    // The picker lists the card form searches by name: every baked character
    // (official + SIF1 import, badged apart) and every skill_center row with
    // its JP and EN display strings
    #[test]
    fn character_and_skill_center_choices_are_wellformed() {
        assert!(CHARACTER_CHOICES.len() > 200, "got {}", CHARACTER_CHOICES.len());
        let honoka = CHARACTER_CHOICES.members().find(|row| row["id"] == 1001).unwrap();
        assert_eq!(honoka["name_en"].as_str(), Some("Honoka Kosaka"));
        assert_eq!(honoka["category"].as_str(), Some("official"));
        let imported = CHARACTER_CHOICES.members().find(|row| row["id"] == 5153).unwrap();
        assert_eq!(imported["category"].as_str(), Some("imported"));
        for row in CHARACTER_CHOICES.members() {
            assert!(row["id"].as_i64().unwrap() > 0);
            assert!(!row["name"].to_string().is_empty());
        }

        assert!(SKILL_CENTER_CHOICES.len() > 60, "got {}", SKILL_CENTER_CHOICES.len());
        let first = SKILL_CENTER_CHOICES.members().find(|row| row["id"] == 100001).unwrap();
        assert_eq!(first["name_en"].as_str(), Some("Smile Heart"));
        assert_eq!(first["detail_text_en"].as_str(), Some("Smile points increased by 3%"));
        assert!(!first["name"].to_string().is_empty());
        assert!(!first["detail_text"].to_string().is_empty());
    }
}

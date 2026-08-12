use actix_web::{web, HttpRequest, HttpResponse, http::header::ContentType};
use actix_multipart::Multipart;
use futures_util::TryStreamExt;
use jzon::{array, object, JsonValue};
use include_dir::{include_dir, Dir};
use std::collections::{HashMap, HashSet};

use crate::router::{global, userdata, webui};
use crate::database::{announcements, permissions};
use crate::database::announcements::Banner;

static ASSETS: Dir<'_> = include_dir!("web_assets/announcement/");

const MAX_BANNER_BYTES: usize = 8 * 1024 * 1024;
const MAX_BANNER_DIM: u32 = 8192;
const MAX_SCALED_PIXELS: u64 = 16 * 1024 * 1024;
const BANNER_W: u32 = 420;
const BANNER_H: u32 = 168;

const CATEGORY_LABELS: &[(i64, &str, &str)] = &[
    (1, "notice", "お知らせ"),
    (2, "update", "アップデート"),
    (3, "bug", "不具合")
];

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/web/announcement")
            .route("", web::get().to(list))
            .route("/detail", web::get().to(detail))
            .route("/bulkRead", web::get().to(bulk_read))
            .route("/assets/{file}", web::get().to(asset))
            .route("/banner/{id}", web::get().to(banner_image))
    );
    cfg.service(
        web::scope("/announcement")
            .route("/list", web::get().to(admin_list))
            .route("/create", web::post().to(create))
            .route("/update", web::post().to(update))
            .route("/delete", web::post().to(delete))
            .route("/banner/{id}", web::get().to(admin_banner_image))
    );
}

fn disabled() -> bool {
    crate::get_args().hidden
}

fn query_i64(req: &HttpRequest, key: &str, def: i64) -> i64 {
    req.query_string()
        .split('&')
        .find(|s| s.starts_with(&format!("{key}=")))
        .and_then(|s| s.split('=').nth(1))
        .and_then(|v| v.parse::<i64>().ok())
        .unwrap_or(def)
}

fn player_key(req: &HttpRequest) -> Option<String> {
    let key = global::get_login(req.headers(), "");
    if key.is_empty() { None } else { Some(key) }
}

fn read_set(req: &HttpRequest) -> HashSet<i64> {
    match player_key(req) {
    Some(key) => { println!("Player has key"); userdata::get_acc_home(&key)["home"]["read_announcement_ids"].members().filter_map(|v| v.as_i64()).collect()},
        None => { println!("No player key"); HashSet::new() }
    }
}

fn mark_read(key: &str, ids: &[i64]) {
    let mut user = userdata::get_acc_home(key);
    let mut set: HashSet<i64> = user["home"]["read_announcement_ids"].members().filter_map(|v| v.as_i64()).collect();
    for id in ids {
        set.insert(*id);
    }
    let mut sorted: Vec<i64> = set.into_iter().collect();
    sorted.sort();
    let mut arr = array![];
    for id in sorted {
        arr.push(id).unwrap();
    }
    user["home"]["read_announcement_ids"] = arr;
    userdata::save_acc_home(key, user);
}

fn display_date(published_at: i64) -> String {
    if published_at <= 0 {
        return String::new();
    }
    let s = global::format_datetime(published_at as u64);
    format!("{}/{}/{} {}", &s[0..4], &s[5..7], &s[8..10], &s[11..16])
}

fn page_head() -> String {
    String::from(r#"<!DOCTYPE html>n<html lang="ja-JP"><head>
<meta http-equiv="content-type" content="text/html; charset=UTF-8">
<meta charset="utf-8">
<meta name="viewport" content="width=320, initial-scale=1.0, user-scalable=no">
<meta name="format-detection" content="telephone=no">
<meta name="robots" content="noindex,nofollow">
<link rel="stylesheet" href="/web/announcement/assets/sanitize.css" type="text/css">
<link rel="stylesheet" href="/web/announcement/assets/news_common.css" type="text/css">
<style>#tab div.on{background-image:none;background-color:#f93981;border-radius:0.6vw;}#news_list hr,#detail hr{background-image:none;background-color:#dfe6e6;}</style>
<script src="/web/announcement/assets/jquery-3.6.0.min.js"></script>
<script>var clicked=false;$(function(){$("a.once").on('click',function(){if(clicked){return false;}clicked=true;return true;});});window.addEventListener('pageshow',function(e){if(e.persisted){clicked=false;}});</script>
</head><body>"#)
}

fn page_foot() -> String {
    String::from("</body></html>")
}

fn tab_bar(active: i64, read: &HashSet<i64>, bulk: bool) -> String {
    let mut tabs = String::new();
    for (cat, class, label) in CATEGORY_LABELS {
        let unread = !bulk && announcements::visible_ids(Some(*cat)).iter().any(|id| !read.contains(id));
        let badge = if unread {
            String::from("<span class=\"new\"><img class=\"bg_badge_eff\" src=\"/web/announcement/assets/news_bg_badge_eff.png\"><img class=\"bg_badge\" src=\"/web/announcement/assets/news_bg_badge.png\"></span>")
        } else {
            String::new()
        };
        let inner = if *cat == active {
            format!("<div class=\"on\">{label}</div>")
        } else {
            format!("<a class=\"once\" href=\"/web/announcement?category={cat}\">{label}</a>")
        };
        tabs.push_str(&format!("<div class=\"{class}\">{badge}{inner}</div>"));
    }
    let read_btn = if bulk {
        String::from("<div class=\"read_btn\"><img class=\"off\" src=\"/web/announcement/assets/news_btn_bulk.png\"></div>")
    } else {
        format!("<div class=\"read_btn\"><a class=\"once\" href=\"/web/announcement/bulkRead?category={active}&amp;page=1\"><img src=\"/web/announcement/assets/news_btn_bulk.png\"></a></div>")
    };
    format!("<div id=\"header\"><div id=\"tab\">{tabs}{read_btn}</div></div><div id=\"tab_bottom\"></div>")
}

fn render_list(category: i64, read: &HashSet<i64>, bulk: bool) -> String {
    let mut list = String::new();
    let items = announcements::list_category(category);
    if items.is_empty() {
        list.push_str(&format!(r#"
<div class="info_title"><span class="title_text">No announcements right now</span></div>
"#));
    }

    for item in items.members() {
        let id = item["id"].as_i64().unwrap_or(0);
        let banner_url = if item["has_banner"].as_bool().unwrap_or(false) {
            format!("/web/announcement/banner/{id}.png")
        } else {
            String::from("/web/announcement/assets/news_banner_generic_news.png")
        };
        let kind = item["type"].as_str().unwrap_or("news");
        let date = display_date(item["published_at"].as_i64().unwrap_or(0));
        let update_text = if item["updated"].as_bool().unwrap_or(false) { "<span class=\"update_text\">- update</span>" } else { "" };
        let new_badge = if !bulk && !read.contains(&id) {
            String::from("<div class=\"info_new_image\"><img class=\"new\" src=\"/web/announcement/assets/news_icon_new.png\"></div>")
        } else {
            String::new()
        };
        let title = item["title"].as_str().unwrap_or("");
        list.push_str(&format!(r#"
<li class="list_area"><div class="information_area"><div id="{id}" class="anchor"></div>
<a href="/web/announcement/detail?announcement_id={id}&amp;page=1" class="news">
<div class="main_image"><span class="banner"><img src="{banner_url}"></span></div>
<div class="information">
<div class="info_type_image"><img class="tag" src="/web/announcement/assets/news_icon_{kind}.png"></div>
<div class="info_date"><span class="date_text">{date}</span>{update_text}</div>
{new_badge}
<div class="clear"></div>
<div class="info_title"><span class="title_text">{title}</span></div>
</div>
<div class="arrow_image"><img class="arrow" src="/web/announcement/assets/news_img_arrow.png"></div>
</a></div><div class="clear"></div><hr></li>
"#));
    }
    
    format!("{}{}<div id=\"news_list\"><ul>{}</ul><div id=\"page_area\"><div id=\"paging\"><span class=\"page off\">1</span></div></div></div>{}",
        page_head(), tab_bar(category, read, bulk), list, page_foot())
}

fn render_detail(item: &JsonValue, read: &HashSet<i64>) -> String {
    let category = item["category"].as_i64().unwrap_or(1);
    let title = item["title"].as_str().unwrap_or("");
    let date = display_date(item["published_at"].as_i64().unwrap_or(0));
    let body = item["body"].as_str().unwrap_or("");
    let banner = if item["has_banner"].as_bool().unwrap_or(false) {
        let id = item["id"].as_i64().unwrap_or(0);
        format!("<div class=\"detail_image\" style=\"display:block\"><img src=\"/web/announcement/banner/{id}.png\"></div>")
    } else {
        String::new()
    };
    format!("{}{}<div id=\"detail\"><div class=\"detail_text\"><div class=\"title\">{title}</div><div class=\"date\">{date}</div>{banner}<hr class=\"hr_detail\">{body}</div></div>{}",
        page_head(), tab_bar(category, read, false), page_foot())
}

fn html(body: String) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType::html())
        .body(body)
}

fn redirect_list(category: i64) -> HttpResponse {
    HttpResponse::Found()
        .insert_header(("Location", format!("/web/announcement?category={category}")))
        .body("")
}

async fn list(req: HttpRequest) -> HttpResponse {
    let category = query_i64(&req, "category", 1);
    let category = if announcements::is_valid_category(category) { category } else { 1 };
    html(render_list(category, &read_set(&req), false))
}

async fn detail(req: HttpRequest) -> HttpResponse {
    let id = query_i64(&req, "announcement_id", 0);
    let Some(item) = announcements::get(id) else {
        return redirect_list(1);
    };
    if !item["visible"].as_bool().unwrap_or(false) {
        return redirect_list(item["category"].as_i64().unwrap_or(1));
    }
    if let Some(key) = player_key(&req) {
        mark_read(&key, &[id]);
    }
    html(render_detail(&item, &read_set(&req)))
}

async fn bulk_read(req: HttpRequest) -> HttpResponse {
    let category = query_i64(&req, "category", 1);
    let category = if announcements::is_valid_category(category) { category } else { 1 };
    if let Some(key) = player_key(&req) {
        mark_read(&key, &announcements::visible_ids(Some(category)));
    }
    html(render_list(category, &read_set(&req), true))
}

async fn asset(req: HttpRequest) -> HttpResponse {
    let file = req.match_info().get("file").unwrap_or("");
    let Some(file) = ASSETS.get_file(file) else {
        return HttpResponse::NotFound().finish();
    };
    let body = file.contents();
    let mime = mime_guess::from_path(file.path()).first_or_octet_stream();
    HttpResponse::Ok()
        .insert_header(ContentType(mime))
        .insert_header(("content-length", body.len()))
        .body(body)
}

fn png_response(bytes: Option<Vec<u8>>) -> HttpResponse {
    match bytes {
        Some(bytes) => HttpResponse::Ok()
            .insert_header(ContentType::png())
            .insert_header(("content-length", bytes.len()))
            .body(bytes),
        None => HttpResponse::NotFound().finish()
    }
}

fn banner_id(req: &HttpRequest) -> i64 {
    req.match_info().get("id").unwrap_or("").trim_end_matches(".png").parse::<i64>().unwrap_or(0)
}

async fn banner_image(req: HttpRequest) -> HttpResponse {
    png_response(announcements::get_public_banner(banner_id(&req)))
}

async fn admin_banner_image(req: HttpRequest) -> HttpResponse {
    if disabled() {
        return HttpResponse::NotFound().finish();
    }
    if manager_uid(&req).filter(|uid| permissions::has(*uid, permissions::ANNOUNCEMENT_MANAGE)).is_none() {
        return HttpResponse::NotFound().finish();
    }
    png_response(announcements::get_banner(banner_id(&req)))
}

type Fields = HashMap<String, Vec<u8>>;

async fn read_multipart(mut payload: Multipart) -> Result<Fields, String> {
    let mut fields = Fields::new();
    let mut total = 0usize;
    while let Some(mut field) = payload.try_next().await.map_err(|e| e.to_string())? {
        let name = field.name().unwrap_or("").to_string();
        let mut data = Vec::new();
        while let Some(chunk) = field.try_next().await.map_err(|e| e.to_string())? {
            total += chunk.len();
            if total > MAX_BANNER_BYTES {
                return Err(format!("Upload exceeds the {} MB limit", MAX_BANNER_BYTES / (1024 * 1024)));
            }
            data.extend_from_slice(&chunk);
        }
        fields.insert(name, data);
    }
    Ok(fields)
}

fn field_str(fields: &Fields, key: &str) -> String {
    String::from_utf8_lossy(fields.get(key).map(|v| v.as_slice()).unwrap_or(&[])).trim().to_string()
}

fn field_flag(fields: &Fields, key: &str) -> bool {
    matches!(field_str(fields, key).to_lowercase().as_str(), "1" | "true" | "on")
}

fn file_of<'a>(fields: &'a Fields, key: &str) -> Option<&'a Vec<u8>> {
    fields.get(key).filter(|v| !v.is_empty())
}

fn process_banner(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let img = image::load_from_memory(bytes).map_err(|_| String::from("The banner is not a decodable image (png, jpg and webp work)"))?;
    if img.width() > MAX_BANNER_DIM || img.height() > MAX_BANNER_DIM {
        return Err(format!("The banner is {}x{} - neither side may exceed {}px", img.width(), img.height(), MAX_BANNER_DIM));
    }
    let img = img.to_rgba8();
    let scale = f64::max(BANNER_W as f64 / img.width() as f64, BANNER_H as f64 / img.height() as f64);
    let scaled_w = ((img.width() as f64 * scale).round() as u32).max(1);
    let scaled_h = ((img.height() as f64 * scale).round() as u32).max(1);
    if (scaled_w as u64) * (scaled_h as u64) > MAX_SCALED_PIXELS {
        return Err(format!("The banner is too far from the {}x{} banner shape to be cropped", BANNER_W, BANNER_H));
    }
    let scaled = image::imageops::resize(&img, scaled_w, scaled_h, image::imageops::FilterType::Lanczos3);
    let x = (scaled.width() - BANNER_W.min(scaled.width())) / 2;
    let y = (scaled.height() - BANNER_H.min(scaled.height())) / 2;
    let cropped = image::imageops::crop_imm(&scaled, x, y, BANNER_W, BANNER_H).to_image();
    let mut rv = Vec::new();
    image::DynamicImage::ImageRgba8(cropped).write_to(&mut std::io::Cursor::new(&mut rv), image::ImageFormat::Png).map_err(|e| e.to_string())?;
    Ok(rv)
}

fn send_json(resp: JsonValue) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType::json())
        .body(jzon::stringify(resp))
}

fn manager_uid(req: &HttpRequest) -> Option<i64> {
    let token = webui::get_login_token(req)?;
    let login_token = userdata::webui_login_token(&token)?;
    userdata::get_acc(&login_token)["user"]["id"].as_i64()
}

fn require_manager(req: &HttpRequest) -> Result<i64, HttpResponse> {
    if disabled() {
        return Err(HttpResponse::NotFound().finish());
    }
    let Some(uid) = manager_uid(req) else {
        return Err(webui::error("Not logged in"));
    };
    if !permissions::has(uid, permissions::ANNOUNCEMENT_MANAGE) {
        return Err(webui::error("You do not have permission to manage announcements"));
    }
    Ok(uid)
}

async fn admin_list(req: HttpRequest) -> HttpResponse {
    if let Err(resp) = require_manager(&req) {
        return resp;
    }
    let mut categories = array![];
    for (id, key, label) in CATEGORY_LABELS {
        categories.push(object!{ id: *id, key: *key, label: *label }).unwrap();
    }
    let mut types = array![];
    for kind in announcements::TYPES {
        types.push(*kind).unwrap();
    }
    send_json(object!{
        result: "OK",
        data: {
            announcements: announcements::get_all(),
            categories: categories,
            types: types
        }
    })
}

async fn create(req: HttpRequest, payload: Multipart) -> HttpResponse {
    let uid = match require_manager(&req) {
        Ok(uid) => uid,
        Err(resp) => return resp
    };
    let fields = match read_multipart(payload).await {
        Ok(fields) => fields,
        Err(e) => return webui::error(&e)
    };
    match save_new(uid, &fields) {
        Ok(id) => send_json(object!{ result: "OK", id: id }),
        Err(e) => webui::error(&e)
    }
}

fn published_at_of(raw: &str) -> i64 {
    if raw.is_empty() {
        global::timestamp() as i64
    } else {
        global::parse_datetime(raw).map(|t| t as i64).unwrap_or_else(|| global::timestamp() as i64)
    }
}

fn save_new(uid: i64, fields: &Fields) -> Result<i64, String> {
    let category = field_str(fields, "category").parse::<i64>().unwrap_or(0);
    if !announcements::is_valid_category(category) {
        return Err(String::from("Invalid category"));
    }
    let kind = field_str(fields, "type");
    if !announcements::is_valid_type(&kind) {
        return Err(String::from("Invalid type"));
    }
    let title = field_str(fields, "title");
    if title.is_empty() {
        return Err(String::from("A title is required"));
    }
    let body = field_str(fields, "body");
    let banner = match file_of(fields, "banner") {
        Some(bytes) => Some(process_banner(bytes)?),
        None => None
    };
    Ok(announcements::create(category, &kind, &title, &body, banner, field_flag(fields, "updated"), !field_flag(fields, "hidden"), published_at_of(&field_str(fields, "published_at")), uid))
}

async fn update(req: HttpRequest, payload: Multipart) -> HttpResponse {
    if let Err(resp) = require_manager(&req) {
        return resp;
    }
    let fields = match read_multipart(payload).await {
        Ok(fields) => fields,
        Err(e) => return webui::error(&e)
    };
    match save_update(&fields) {
        Ok(id) => send_json(object!{ result: "OK", id: id }),
        Err(e) => webui::error(&e)
    }
}

fn save_update(fields: &Fields) -> Result<i64, String> {
    let id = field_str(fields, "id").parse::<i64>().unwrap_or(0);
    let Some(stored) = announcements::get(id) else {
        return Err(String::from("That announcement no longer exists"));
    };
    let category = field_str(fields, "category").parse::<i64>().unwrap_or(0);
    if !announcements::is_valid_category(category) {
        return Err(String::from("Invalid category"));
    }
    let kind = field_str(fields, "type");
    if !announcements::is_valid_type(&kind) {
        return Err(String::from("Invalid type"));
    }
    let title = field_str(fields, "title");
    if title.is_empty() {
        return Err(String::from("A title is required"));
    }
    let body = field_str(fields, "body");
    let banner = if let Some(bytes) = file_of(fields, "banner") {
        Banner::Set(process_banner(bytes)?)
    } else if field_flag(fields, "remove_banner") {
        Banner::Clear
    } else {
        Banner::Keep
    };
    let published_at = if field_str(fields, "published_at").is_empty() {
        stored["published_at"].as_i64().unwrap_or_else(|| global::timestamp() as i64)
    } else {
        published_at_of(&field_str(fields, "published_at"))
    };
    announcements::update(id, category, &kind, &title, &body, banner, field_flag(fields, "updated"), !field_flag(fields, "hidden"), published_at);
    Ok(id)
}

async fn delete(req: HttpRequest, body: String) -> HttpResponse {
    if let Err(resp) = require_manager(&req) {
        return resp;
    }
    let body = jzon::parse(&body).unwrap_or(object!{});
    let id = body["id"].as_i64().unwrap_or(0);
    if announcements::get(id).is_none() {
        return webui::error("That announcement no longer exists");
    }
    announcements::delete(id);
    send_json(object!{ result: "OK" })
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_pixel(w, h, image::Rgba([120, 90, 200, 255]));
        let mut rv = Vec::new();
        image::DynamicImage::ImageRgba8(img).write_to(&mut std::io::Cursor::new(&mut rv), image::ImageFormat::Png).unwrap();
        rv
    }

    #[test]
    fn banners_are_cropped_to_the_official_size() {
        for (w, h) in [(420, 168), (1200, 300), (300, 1200), (64, 64)] {
            let out = process_banner(&png(w, h)).unwrap();
            let decoded = image::load_from_memory(&out).unwrap();
            assert_eq!((decoded.width(), decoded.height()), (BANNER_W, BANNER_H), "source {}x{}", w, h);
        }
        assert!(process_banner(b"not an image").unwrap_err().contains("not a decodable image"));
    }

    #[test]
    fn extreme_sources_are_refused_before_the_resize_allocates() {
        let err = process_banner(&png(9000, 32)).unwrap_err();
        assert!(err.contains("9000x32"), "got {}", err);

        let err = process_banner(&png(24, 6000)).unwrap_err();
        assert!(err.contains("banner shape"), "got {}", err);
    }
}

use actix_web::{web, HttpRequest, HttpResponse, http::header::ContentType};
use jzon::{object, JsonValue};
use crate::database::{custom_group, permissions};
use crate::router::{userdata, webui, rich_text};
use std::io::Cursor;
use futures_util::StreamExt;

pub const PROTOCOL_VERSION: u32 = 6;

pub fn web_routes(cfg: &mut web::ServiceConfig) {
    cfg.service(web::scope("/custom_group")
        .route("/list", web::get().to(list))
        .route("/create", web::post().to(create))
        .route("/{id}/logo", web::post().to(upload_logo))
        .route("/data/{hash}/{file}", web::get().to(logo_data)));
}

fn can_manage(req: &HttpRequest) -> bool {
    webui::get_login_token(req)
        .and_then(|token| userdata::webui_login_token(&token))
        .and_then(|key| userdata::get_acc(&key)["user"]["id"].as_i64())
        .is_some_and(|id| permissions::has(id, permissions::GROUP_MANAGE))
}

fn encode_logo(bytes: &[u8]) -> Result<Vec<u8>, String> {
    let mut reader = image::ImageReader::new(Cursor::new(bytes))
        .with_guessed_format().map_err(|_| "Invalid image")?;
    if !matches!(reader.format(), Some(image::ImageFormat::Png | image::ImageFormat::Jpeg)) {
        return Err("Logo must be PNG or JPEG".into());
    }
    let mut limits = image::Limits::default();
    limits.max_image_width = Some(4096);
    limits.max_image_height = Some(4096);
    limits.max_alloc = Some(64 * 1024 * 1024);
    reader.limits(limits);
    let image = reader.decode().map_err(|_| "Invalid or oversized image")?.thumbnail(512, 256);
    let mut png = Cursor::new(Vec::new());
    image.write_to(&mut png, image::ImageFormat::Png).map_err(|_| "Could not encode logo")?;
    Ok(png.into_inner())
}

async fn upload_logo(req: HttpRequest, id: web::Path<i64>, mut payload: web::Payload) -> HttpResponse {
    if !can_manage(&req) { return HttpResponse::Forbidden().finish(); }
    if !custom_group::exists(*id) { return HttpResponse::NotFound().finish(); }
    let mut bytes = Vec::new();
    while let Some(chunk) = payload.next().await {
        let Ok(chunk) = chunk else { return HttpResponse::BadRequest().finish(); };
        if bytes.len() + chunk.len() > 4 * 1024 * 1024 { return HttpResponse::PayloadTooLarge().finish(); }
        bytes.extend_from_slice(&chunk);
    }
    let png = match web::block(move || encode_logo(&bytes)).await {
        Ok(Ok(png)) => png,
        Ok(Err(message)) => return reply(object! { "result": "ERR", "message": message }),
        Err(_) => return HttpResponse::InternalServerError().finish(),
    };
    let hash = format!("{:x}", md5::compute(&png));
    let dir = crate::runtime::get_data_path("custom_groups");
    if store_logo(&dir, &hash, &png).is_err()
        || custom_group::set_logo(*id, &hash, png.len() as i64).is_err() {
        return HttpResponse::InternalServerError().finish();
    }
    crate::database::custom_song::bump_revision();
    crate::database::custom_card::bump_revision();
    reply(object! { "result": "OK", "logo_md5": hash, "logo_size": png.len() })
}

fn store_logo(dir: &str, hash: &str, png: &[u8]) -> std::io::Result<()> {
    std::fs::create_dir_all(dir)?;
    let destination = format!("{dir}/{hash}.png");
    if std::fs::read(&destination).is_ok_and(|bytes| bytes == png) { return Ok(()); }
    let temporary = format!("{dir}/{}.tmp", uuid::Uuid::now_v7());
    let result = std::fs::write(&temporary, png).and_then(|_| std::fs::rename(&temporary, destination));
    if result.is_err() { let _ = std::fs::remove_file(temporary); }
    result
}

async fn logo_data(path: web::Path<(String, String)>) -> HttpResponse {
    let (hash, file) = path.into_inner();
    if hash.len() != 32 || !hash.bytes().all(|b| b.is_ascii_hexdigit())
        || file != format!("{hash}.png") || !custom_group::has_logo(&hash) {
        return HttpResponse::NotFound().finish();
    }
    match std::fs::read(crate::runtime::get_data_path(&format!("custom_groups/{hash}.png"))) {
        Ok(bytes) if format!("{:x}", md5::compute(&bytes)) == hash => HttpResponse::Ok()
            .insert_header(("Cache-Control", "public, max-age=31536000, immutable"))
            .content_type("image/png").body(bytes),
        _ => HttpResponse::NotFound().finish(),
    }
}

fn reply(value: JsonValue) -> HttpResponse {
    HttpResponse::Ok().insert_header(ContentType::json()).body(jzon::stringify(value))
}

async fn list() -> HttpResponse {
    reply(object! { "result": "OK", "groups": custom_group::list() })
}

async fn create(req: HttpRequest, body: String) -> HttpResponse {
    if !can_manage(&req) {
        return reply(object! { "result": "ERR", "message": "You do not have permission to create groups" });
    }
    let Ok(data) = jzon::parse(&body) else {
        return reply(object! { "result": "ERR", "message": "Invalid JSON" });
    };
    let name = data["name"].as_str().unwrap_or("").trim();
    let name_en = data["name_en"].as_str().unwrap_or("").trim();
    if name.is_empty() || name.chars().count() > 60 || name_en.chars().count() > 60 {
        return reply(object! { "result": "ERR", "message": "Group name must be 1-60 characters (English name at most 60)" });
    }
    if let Err(message) = rich_text::reject_tags("Group name", name, &[]) {
        return reply(object! { "result": "ERR", "message": message });
    }
    if let Err(message) = rich_text::reject_tags("English group name", name_en, &[]) {
        return reply(object! { "result": "ERR", "message": message });
    }
    match custom_group::create(name, name_en) {
        Ok(id) => reply(object! { "result": "OK", "id": id }),
        Err(_) => reply(object! { "result": "ERR", "message": "A group with that name already exists, or it could not be stored" })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn logo_preserves_aspect_and_transparency() {
        let source = image::DynamicImage::ImageRgba8(image::RgbaImage::from_pixel(1024, 256, image::Rgba([1, 2, 3, 64])));
        let mut input = Cursor::new(Vec::new());
        source.write_to(&mut input, image::ImageFormat::Png).unwrap();
        let output = encode_logo(input.get_ref()).unwrap();
        let decoded = image::load_from_memory(&output).unwrap().to_rgba8();
        assert_eq!(decoded.dimensions(), (512, 128));
        assert_eq!(decoded.get_pixel(0, 0).0[3], 64);
        assert!(encode_logo(b"not an image").is_err());
    }

    #[test]
    fn rejects_oversized_dimensions() {
        let source = image::DynamicImage::new_rgba8(4097, 1);
        let mut input = Cursor::new(Vec::new());
        source.write_to(&mut input, image::ImageFormat::Png).unwrap();
        assert!(encode_logo(input.get_ref()).is_err());
    }

    #[actix_web::test]
    async fn logo_upload_requires_group_permission() {
        let app = actix_web::test::init_service(actix_web::App::new().configure(web_routes)).await;
        let request = actix_web::test::TestRequest::post().uri("/custom_group/10000/logo").set_payload("invalid").to_request();
        assert_eq!(actix_web::test::call_service(&app, request).await.status(), actix_web::http::StatusCode::FORBIDDEN);
    }

    #[actix_web::test]
    async fn logo_catalog_and_content_addressed_download() {
        let _lock = crate::runtime::lock_test_data_path();
        let id = custom_group::create(&format!("Logo test {}", uuid::Uuid::now_v7()), "").unwrap();
        let mut input = Cursor::new(Vec::new());
        image::DynamicImage::new_rgba8(32, 16).write_to(&mut input, image::ImageFormat::Png).unwrap();
        let png = encode_logo(input.get_ref()).unwrap();
        let hash = format!("{:x}", md5::compute(&png));
        let dir = crate::runtime::get_data_path("custom_groups");
        store_logo(&dir, &hash, &png).unwrap();
        store_logo(&dir, &hash, &png).unwrap();
        custom_group::set_logo(id, &hash, png.len() as i64).unwrap();
        let catalog = custom_group::list();
        let row = catalog.members().find(|g| g["id"].as_i64() == Some(id)).unwrap();
        assert_eq!(row["logo_md5"].as_str(), Some(hash.as_str()));
        assert_eq!(row["logo_size"].as_usize(), Some(png.len()));
        let app = actix_web::test::init_service(actix_web::App::new().configure(web_routes)).await;
        let request = actix_web::test::TestRequest::get().uri(&format!("/custom_group/data/{hash}/{hash}.png")).to_request();
        let response = actix_web::test::call_service(&app, request).await;
        assert_eq!(response.status(), actix_web::http::StatusCode::OK);
        assert_eq!(actix_web::test::read_body(response).await.as_ref(), png);
        let request = actix_web::test::TestRequest::get().uri(&format!("/custom_group/data/{hash}/wrong.png")).to_request();
        assert_eq!(actix_web::test::call_service(&app, request).await.status(), actix_web::http::StatusCode::NOT_FOUND);
        std::fs::write(format!("{dir}/{hash}.png"), b"corrupted").unwrap();
        let request = actix_web::test::TestRequest::get().uri(&format!("/custom_group/data/{hash}/{hash}.png")).to_request();
        assert_eq!(actix_web::test::call_service(&app, request).await.status(), actix_web::http::StatusCode::NOT_FOUND);
    }
}

use actix_web::{get, HttpResponse, HttpRequest, http::header::ContentType, web, guard};
use std::fs;
use std::path::{Path, PathBuf};

#[get("/maintenance/maintenance.json")]
async fn maintenance(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::APPLICATION_JSON))
        .body(r#"{"opened_at":"2024-02-05 02:00:00","closed_at":"2024-02-05 04:00:00","message":":(","server":1,"gamelib":0}"#)
}

fn safe_join(base: &Path, untrusted: &str) -> Option<PathBuf> {
    let relative = untrusted.trim_start_matches("/");
    if Path::new(relative)
        .components()
        .any(|c| matches!(c, std::path::Component::ParentDir))
    {
        return None;
    }
    Some(base.join(relative))
}

#[cfg(feature = "library")]
use include_dir::{include_dir, Dir};

#[cfg(all(feature = "library", target_os = "ios"))]
static SPART_FILES: Dir<'_> = include_dir!("assets/iOS/");

#[cfg(all(feature = "library", target_os = "android"))]
static SPART_FILES: Dir<'_> = include_dir!("assets/Android/");

fn handle_assets(req: HttpRequest) -> HttpResponse {
    #[cfg(feature = "library")]
    {
        let lang: String = req.match_info().get("lang").unwrap_or("JP").parse().unwrap_or(String::from("JP"));
        let file_name: String = req.match_info().get("file").unwrap().parse().unwrap();
        let hash: String = req.match_info().get("file").unwrap().parse().unwrap();
        if let Some(file) = SPART_FILES.get_file(format!("{lang}/{hash}/{file_name}")) {
            let body = file.contents();
            return HttpResponse::Ok()
                .insert_header(ContentType(mime::APPLICATION_OCTET_STREAM))
                .insert_header(("content-length", body.len()))
                .body(body);
        }
    }

    let assets_root = Path::new("assets");

    let Some(file_path) = safe_join(assets_root, req.path()) else {
        return HttpResponse::BadRequest().body("Invalid path");
    };

    match fs::read(&file_path) {
        Ok(contents) => HttpResponse::Ok().body(contents),
        Err(_) => HttpResponse::SeeOther()
            .insert_header(("location", format!("https://sif2.sif.moe{}", req.path())))
            .body(""),
    }
}

async fn files_jp(req: HttpRequest) -> HttpResponse {
    handle_assets(req)
}

async fn files_gl(req: HttpRequest) -> HttpResponse {
    handle_assets(req)
}

fn platform_guard(ctx: &guard::GuardContext) -> bool {
    let platform = ctx.head().uri.path()
        .split('/')
        .nth(1)
        .unwrap_or("");
    matches!(platform, "Android" | "StandaloneWindows64" | "iOS")
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::resource("/{platform}/{hash}/{file}")
            .guard(guard::fn_guard(platform_guard))
            .route(web::get().to(files_jp))
    )
        .service(
            web::resource("/{platform}/{lang}/{hash}/{file}")
                .guard(guard::fn_guard(platform_guard))
                .route(web::get().to(files_gl))
        );
}

use actix_web::{HttpRequest, web, Responder, HttpResponse};
use actix_web::http::header::ContentType;
use jzon::object;
use lazy_static::lazy_static;
use std::collections::HashMap;
use std::sync::Mutex;
use crate::include_file;

lazy_static! {
    static ref MERGED_CACHE: Mutex<HashMap<String, String>> = Mutex::new(HashMap::new());
}

pub fn routes(cfg: &mut web::ServiceConfig) {
    cfg.service(
        web::scope("/assetLists")
            .route("/supported", web::get().to(supported))
            .route("{platform}/{LANG}", web::get().to(get))
    );
}

async fn get(_req: HttpRequest) -> impl Responder {
    let mut response = object!{};
    response["Bundle"] = load_list("Bundle").into();
    response["Movie"] = load_list("Movie").into();
    response["Sound"] = load_list("Sound").into();

    let body = jzon::stringify(response);
    HttpResponse::Ok()
        .insert_header(("content-type", ContentType::json()))
        .insert_header(("content-length", body.len()))
        .body(body)
}

fn load_list(name: &str) -> String {
    if let Some(cached) = MERGED_CACHE.lock().unwrap().get(name) {
        return cached.clone();
    }
    let rel = format!("asset_lists/{}.json", name);
    let base = crate::runtime::read_masterdata_file(&rel)
        .and_then(|b| String::from_utf8(b).ok())
        .unwrap_or_else(|| match name {
            "Bundle" => include_file!("src/router/asset_lists/Bundle.json"),
            "Movie"  => include_file!("src/router/asset_lists/Movie.json"),
            "Sound"  => include_file!("src/router/asset_lists/Sound.json"),
            _ => unreachable!(),
        });

    let mod_files = crate::runtime::read_mod_files(&rel);
    let merged = if mod_files.is_empty() {
        base
    } else {
        merge_asset_list(name, base, mod_files)
    };
    MERGED_CACHE.lock().unwrap().insert(name.to_string(), merged.clone());
    merged
}

fn merge_asset_list(name: &str, base: String, mod_files: Vec<(String, Vec<u8>)>) -> String {
    let mut root = match jzon::parse(&base) {
        Ok(v) => v,
        Err(_) => return base,
    };

    let mut by_ident: HashMap<String, usize> = HashMap::new();
    if let jzon::JsonValue::Array(ref arr) = root["m_manifestCollection"] {
        for (i, e) in arr.iter().enumerate() {
            let id = e["m_identifier"].to_string();
            by_ident.insert(id.to_string(), i);
        }
    }

    for (mod_dir, bytes) in mod_files {
        let Ok(s) = String::from_utf8(bytes) else { continue };
        let Ok(mod_root) = jzon::parse(&s) else { continue };
        let mod_entries = match mod_root["m_manifestCollection"] {
            jzon::JsonValue::Array(ref arr) => arr.clone(),
            _ => continue,
        };
        let mut added = 0usize;
        let mut replaced = 0usize;
        for entry in mod_entries {
            let ident = entry["m_identifier"].as_str().unwrap_or("").to_string();
            if ident.is_empty() {
                continue;
            }
            if let Some(&idx) = by_ident.get(&ident) {
                root["m_manifestCollection"][idx] = entry;
                replaced += 1;
            } else {
                let idx = root["m_manifestCollection"].len();
                let _ = root["m_manifestCollection"].push(entry);
                by_ident.insert(ident, idx);
                added += 1;
            }
        }
        if added > 0 || replaced > 0 {
            println!(
                "[mod {}] {}.json: +{} new entries, {} replaced",
                mod_dir, name, added, replaced
            );
        }
    }
    jzon::stringify(root)
}

async fn supported() -> impl Responder {
    "SUPPORTED"
}

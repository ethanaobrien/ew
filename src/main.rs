mod encryption;
mod router;
mod sql;

use actix_web::{
    rt,
    App,
    HttpServer,
    get,
    HttpResponse,
    HttpRequest,
    web,
    dev::Service,
    http::header::ContentType
};
use clap::Parser;
use std::fs;
use std::sync::Mutex;
use std::time::Duration;
use lazy_static::lazy_static;

#[get("/index.css")]
async fn css(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::TEXT_CSS))
        .body(include_file!("webui/dist/index.css"))
}
#[get("/index.js")]
async fn js(_req: HttpRequest) -> HttpResponse {
    HttpResponse::Ok()
        .insert_header(ContentType(mime::APPLICATION_JAVASCRIPT_UTF_8))
        .body(include_file!("webui/dist/index.js"))
}

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
pub struct Args {
    #[arg(short, long, default_value_t = 8080, help = "Port to listen on")]
    port: u16,

    #[arg(long, default_value = "./", help = "Path to store database files")]
    path: String,

    #[arg(long, default_value_t = false, help = "Serve gree headers with https. WILL NOT ACCEPT HTTPS REQUESTS")]
    https: bool,

    #[arg(long, default_value = "http://127.0.0.1:51376", help = "Address to NPPS4 server for sif account linking")]
    npps4: String,

    //below options are for the "Help" page

    #[arg(long, default_value = "", help = "Link to patched android global apk for this server.")]
    global_android: String,

    #[arg(long, default_value = "", help = "Link to patched android japan apk for this server.")]
    japan_android: String,

    #[arg(long, default_value = "", help = "Link to patched iOS global apk for this server.")]
    global_ios: String,

    #[arg(long, default_value = "", help = "Link to patched iOS japan apk for this server.")]
    japan_ios: String,

    #[arg(long, default_value = "", help = "Link to asset server.")]
    assets_url: String,

    #[arg(long, default_value_t = 0, help = "Max time returned by the server, in the JSON \"timestamp\" key.")]
    max_time: u64,

    #[arg(long, default_value_t = false, help = "Disable webui, act completely like the original server")]
    hidden: bool,

    #[arg(long, default_value_t = false, help = "Purge dead user accounts on startup")]
    purge: bool,

    #[arg(long, default_value_t = false, help = "Disable user account imports")]
    disable_imports: bool,

    #[arg(long, default_value_t = false, help = "Disable user account exports")]
    disable_exports: bool,

    #[arg(long, default_value = "", help = "Asset hash for English iOS client.")]
    en_ios_asset_hash: String,

    #[arg(long, default_value = "", help = "Asset hash for JP iOS client.")]
    jp_ios_asset_hash: String,

    #[arg(long, default_value = "", help = "Asset hash for English Android client.")]
    en_android_asset_hash: String,

    #[arg(long, default_value = "", help = "Asset hash for JP Android client.")]
    jp_android_asset_hash: String
}

#[actix_web::main]
async fn run_server(in_thread: bool) -> std::io::Result<()> {
    let args = get_args();
    let port = args.port;

    if args.purge {
        println!("Purging accounts...");
        let ct = crate::router::userdata::purge_accounts();
        println!("Purged {} accounts", ct);
    }

    let rv = HttpServer::new(|| App::new()
    .wrap_fn(|req, srv| {
        println!("Request: {}", req.path());
        srv.call(req)
    })
    .app_data(web::PayloadConfig::default().limit(1024 * 1024 * 25))
    .service(css)
    .service(js)
    .default_service(web::route().to(router::request))
    ).bind(("0.0.0.0", port))?.run();

    println!("Server started: http://0.0.0.0:{}", port);
    println!("Data path is set to {}", args.path);
    println!("Sif1 transfer requests will attempt to contact NPPS4 at {}", args.npps4);

    if args.https {
        println!("Note: gree is set to https mode. http requests will fail on jp clients.");
    }

    if in_thread {
        set_running(true).await;
        let handle = rv.handle();
        rt::spawn(rv);
        while get_running().await {
            actix_web::rt::time::sleep(Duration::from_millis(100)).await;
        }
        handle.stop(false).await;
        println!("Stopped");
        return Ok(());
    }
    rv.await
}

#[actix_web::main]
async fn stop_server() {
    set_running(false).await;
    println!("Stopping");
}

fn main() -> std::io::Result<()> {
    run_server(false)
}

pub fn get_args() -> Args {
    Args::parse()
}

pub fn get_data_path(file_name: &str) -> String {
    let args = get_args();
    let mut path = args.path;
    while path.ends_with('/') {
        path.pop();
    }
    fs::create_dir_all(&path).unwrap();
    format!("{}/{}", path, file_name)
}

// include_file macro: includes a file compressed at compile time, and decompresses it on reference. Decreases binary size
#[macro_export]
macro_rules! include_file {
    ( $s:expr ) => {
        {
            let file = include_flate_codegen::deflate_file!($s);
            let ret = $crate::decode(file);
            std::string::String::from_utf8(ret).unwrap()
        }
    };
}

pub fn decode(bytes: &[u8]) -> Vec<u8> {
    use std::io::{Cursor, Read};

    let mut dec = libflate::deflate::Decoder::new(Cursor::new(bytes));
    let mut ret = Vec::new();
    dec.read_to_end(&mut ret).unwrap();
    ret
}

#[macro_export]
macro_rules! lock_onto_mutex {
    ($mutex:expr) => {{
        loop {
            match $mutex.lock() {
                Ok(value) => {
                    break value;
                }
                Err(_) => {
                    $mutex.clear_poison();
                    actix_web::rt::time::sleep(std::time::Duration::from_millis(15)).await;
                }
            }
        }
    }};
}

lazy_static! {
    static ref RUNNING: Mutex<bool> = Mutex::new(false);
}

async fn set_running(running: bool) {
    let mut result = lock_onto_mutex!(RUNNING);
    *result = running;
}

async fn get_running() -> bool {
    let result = lock_onto_mutex!(RUNNING);
    *result
}

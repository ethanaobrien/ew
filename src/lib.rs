mod static_handlers;
mod options;
mod router;
mod encryption;
mod sql;

use actix_web::{
    rt,
    App,
    HttpServer,
    web,
    dev::Service
};
use std::fs;
use std::sync::Mutex;
use std::time::Duration;
use lazy_static::lazy_static;
use options::get_args;


#[actix_web::main]
pub async fn run_server(in_thread: bool) -> std::io::Result<()> {
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
    .service(static_handlers::css)
    .service(static_handlers::maintenance)
    .service(static_handlers::js)
    .service(static_handlers::files_jp)
    .service(static_handlers::files_gl)
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

pub fn get_data_path(file_name: &str) -> String {
    let args = get_args();
    let mut path = args.path;
    while path.ends_with('/') {
        path.pop();
    }
    fs::create_dir_all(&path).unwrap();
    format!("{}/{}", path, file_name)
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

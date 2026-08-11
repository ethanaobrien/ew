mod static_handlers;
mod options;
mod router;
mod encryption;
mod sql;
pub mod runtime;
pub mod database;
#[macro_use]
mod macros;

#[cfg(feature = "library")]
#[cfg(target_os = "android")]
mod android;

#[cfg(feature = "library")]
#[cfg(target_os = "ios")]
mod ios;

use actix_web::{rt, App, HttpServer, web, dev::Service};
//use actix_cors::Cors;
use std::time::Duration;
pub use options::get_args;
use runtime::get_data_path;

#[actix_web::main]
pub async fn run_server(in_thread: bool) -> std::io::Result<()> {
    let args = get_args();
    let port = args.port;

    runtime::update_owners(&args.owner);

    if args.purge {
        println!("Purging accounts...");
        let ct = crate::router::userdata::purge_accounts();
        println!("Purged {} accounts", ct);
    }

    router::custom_song::migrate::run();
    router::custom_song::sweep_audio();
    // The multi-live relay's expiry timers, on the system arbiter rather than on whichever
    // HTTP worker happened to serve the first WebSocket upgrade — a worker panic must not
    // be able to strand every room's seats for the life of the process.
    router::multi_live::start_sweeper();

    let rv = HttpServer::new(|| App::new()
    //.wrap(Cors::permissive())
    .wrap_fn(|req, srv| {
        println!("Request: {} {}", req.method(), req.path());

        #[cfg(feature = "library")]
        #[cfg(target_os = "android")]
        log_to_logcat!("ew", "Request: {} {}", req.method(), req.path());

        srv.call(req)
    })
    .app_data(web::PayloadConfig::default().limit(1024 * 1024 * 25))
    .service(static_handlers::maintenance)
    .configure(router::configure)
    .default_service(web::route().to(router::request))
    ).bind(("0.0.0.0", port))?.run();

    println!("Server started: http://0.0.0.0:{}", port);
    println!("Data path is set to {}", args.path);
    println!("Sif1 transfer requests will attempt to contact NPPS4 at {}", args.npps4);

    if args.https {
        println!("Note: gree is set to https mode. http requests will fail on jp clients.");
    }

    if in_thread {
        runtime::set_running(true);
        let handle = rv.handle();
        rt::spawn(rv);
        while runtime::get_running() {
            actix_web::rt::time::sleep(Duration::from_millis(100)).await;
        }
        handle.stop(false).await;
        println!("Stopped");
        return Ok(());
    }
    rv.await
}

#[actix_web::main]
pub async fn stop_server() {
    runtime::set_running(false);
    println!("Stopping");
}

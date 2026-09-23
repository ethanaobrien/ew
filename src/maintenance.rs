use actix_web::{web, App, HttpRequest, HttpResponse, HttpServer};
use jzon::object;

#[actix_web::main]
pub async fn run(port: u16, message: String) -> std::io::Result<()> {
    let message = web::Data::new(message);
    let server = HttpServer::new(move || {
        App::new()
            .app_data(message.clone())
            .default_service(web::route().to(respond))
    })
    .bind(("0.0.0.0", port))?
    .run();
    println!("Maintenance server listening on http://0.0.0.0:{port}");
    server.await
}

async fn respond(req: HttpRequest, message: web::Data<String>) -> HttpResponse {
    let message = message.get_ref().as_str();
    if req.path() == "/maintenance/maintenance.json" {
        // The title screen requires HTTP 200, server=false, and an active UTC window.
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .content_type("application/json")
            .body(object! {
                "opened_at": "1970-01-01 00:00:00",
                "closed_at": "9999-01-01 00:00:00",
                "message": message,
                "server": false,
                "gamelib": 0
            }.dump());
    }

    let webui = req.path() == "/api/webui" || req.path().starts_with("/api/webui/");
    let game_api = req.path() == "/api" || req.path().starts_with("/api/");
    let browser = req.headers().get("Accept").and_then(|value| value.to_str().ok())
        .is_some_and(|value| value.contains("text/html"));
    if req.headers().contains_key("aoharu-asset-version") || (game_api && !webui && !browser) {
        // Do not use global::send: its per-user clock can access the database.
        let body = object! {
            "code": 10,
            "server_time": crate::router::global::timestamp(),
            "message": message
        }.dump();
        return HttpResponse::Ok()
            .insert_header(("Cache-Control", "no-store"))
            .body(crate::encryption::encrypt_packet(&body).expect("packet encryption failed"));
    }

    let escaped = message.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;")
        .replace('"', "&quot;").replace('\'', "&#39;");
    HttpResponse::ServiceUnavailable()
        .insert_header(("Cache-Control", "no-store"))
        .content_type("text/html; charset=utf-8")
        .body(include_str!("../web_assets/maintenance.html").replace("{{message}}", &escaped))
}

#[cfg(test)]
mod tests {
    use super::*;
    use actix_web::{http::{Method, StatusCode}, test};

    const MESSAGE: &str = "Updating \"songs\" & <cards> — またね\nPlease wait.";

    #[actix_web::test]
    async fn title_check_is_active_and_preserves_message() {
        let app = test::init_service(App::new().app_data(web::Data::new(MESSAGE.to_string()))
            .default_service(web::route().to(respond))).await;
        let req = test::TestRequest::get().uri("/maintenance/maintenance.json?cache=1").to_request();
        let response = test::call_service(&app, req).await;
        assert_eq!(response.status(), StatusCode::OK);
        assert_eq!(response.headers().get("Cache-Control").unwrap(), "no-store");
        let body = test::read_body(response).await;
        let json = jzon::parse(std::str::from_utf8(&body).unwrap()).unwrap();
        assert_eq!(json["message"].as_str(), Some(MESSAGE));
        assert_eq!(json["server"].as_bool(), Some(false));
        let now = crate::router::global::format_datetime(crate::router::global::timestamp());
        assert!(json["opened_at"].as_str().unwrap() < now.as_str());
        assert!(json["closed_at"].as_str().unwrap() > now.as_str());
    }

    #[actix_web::test]
    async fn game_requests_get_encrypted_maintenance_without_reading_the_body() {
        let app = test::init_service(App::new().app_data(web::Data::new(MESSAGE.to_string()))
            .default_service(web::route().to(respond))).await;
        for (path, header) in [("/api/start", false), ("/api/live/end", true),
            ("/api/unknown", false), ("/v1.0/test", true), ("/anything", true)] {
            let mut req = test::TestRequest::post().uri(path)
                .insert_header(("aoharu-user-id", "12345")).set_payload("not an encrypted request");
            if header { req = req.insert_header(("aoharu-asset-version", "old")); }
            let response = test::call_service(&app, req.to_request()).await;
            assert_eq!(response.status(), StatusCode::OK, "{path}");
            let body = test::read_body(response).await;
            let decoded = crate::encryption::decrypt_packet(std::str::from_utf8(&body).unwrap()).unwrap();
            let json = jzon::parse(&decoded).unwrap();
            assert_eq!(json["code"].as_i32(), Some(10));
            assert_eq!(json["message"].as_str(), Some(MESSAGE));
        }
    }

    #[actix_web::test]
    async fn all_other_routes_and_methods_are_maintenance_only() {
        let app = test::init_service(App::new().app_data(web::Data::new(MESSAGE.to_string()))
            .default_service(web::route().to(respond))).await;
        for path in ["/", "/maintenance.html", "/custom_song/upload", "/custom_card/delete",
            "/custom_3dmv/upload", "/announcement/create", "/api/webui/login",
            "/Android/hash/file", "/v1.0/test", "/api/user", "/unknown"] {
            for method in [Method::GET, Method::POST, Method::PUT, Method::DELETE, Method::OPTIONS] {
                let response = test::call_service(&app,
                    test::TestRequest::default().method(method).uri(path)
                        .insert_header(("Accept", "text/html")).to_request()).await;
                assert_eq!(response.status(), StatusCode::SERVICE_UNAVAILABLE, "{path}");
                let body = test::read_body(response).await;
                let html = std::str::from_utf8(&body).unwrap();
                assert!(html.contains("&lt;cards&gt;"));
                assert!(html.contains("またね"));
                assert!(!html.contains("<cards>"));
            }
        }
    }
}

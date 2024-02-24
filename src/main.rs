mod encryption;
mod router;
use actix_web::{
    post,
    get,
    HttpResponse,
    HttpRequest,
    web,
    dev::Service
};

#[post("/api/start")]
async fn start_start(req: HttpRequest, body: String) -> HttpResponse { router::start::start(req, body) }

#[post("/api/start/assetHash")]
async fn start_assethash(req: HttpRequest, body: String) -> HttpResponse { router::start::asset_hash(req, body) }

#[post("/api/dummy/login")]
async fn dummy_login(req: HttpRequest, body: String) -> HttpResponse { router::login::dummy(req, body) }

#[get("/api/user")]
async fn user(req: HttpRequest) -> HttpResponse { router::user::user(req) }

#[get("/api/purchase")]
async fn purchase(req: HttpRequest) -> HttpResponse { router::purchase::purchase(req) }

#[post("/api/tutorial")]
async fn tutorial(req: HttpRequest, body: String) -> HttpResponse { router::tutorial::tutorial(req, body) }

#[get("/api/mission")]
async fn mission(req: HttpRequest) -> HttpResponse { router::mission::mission(req) }

async fn log_unknown_request(req: HttpRequest) -> HttpResponse {
    println!("Unhandled request: {}", req.path());
    HttpResponse::Ok().body("ok")
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    use actix_web::{App, HttpServer};

    let rv = HttpServer::new(|| App::new()
        .wrap_fn(|req, srv| {
            println!("Request: {}", req.path());
            srv.call(req)
        })
        .service(purchase)
        .service(start_start)
        .service(tutorial)
        .service(mission)
        .service(start_assethash)
        .service(user)
        .service(dummy_login)
        .default_service(web::route().to(log_unknown_request)))
        .bind(("0.0.0.0", 8080))?
        .run();
    println!("Server started: http://127.0.0.1:{}", 8080);
    rv.await
}



/*
fn main() {
    let base64_input = "MX2tzmKTxY7EsV46rYFZuAfxeY0tPHuZ0etG15WsK1MAzs/U0WUXE4bJZINrEvCxqqUbvCYxhDtXp3HoeH/zDXtnW183aF/aYycmUW3aAF6zyio4/PJoqFl7EGET37ruotoQ9Teof2PXpXraF94diw==";
    match decrypt_packet(base64_input) {
        Ok(decrypted_json) => {
            // Process the decrypted JSON
            println!("Decrypted JSON: {}", decrypted_json);
        }
        Err(err) => {
            eprintln!("Error decrypting packet: {}", err);
        }
    }
}

*/

/*

async fn make_post_request(url: &str, body: &str, headers: &HeaderMap) -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();
    let mut response = client
        .post(url)
        .body(body.to_string());
    
    for (name, value) in headers.iter() {
        if name == "Accept-Encoding" {continue;};
        if name == "host" {
            response = response.header("host", "api-sif2.lovelive-sif2.com");
            continue;
        };
        println!("{}: {}", name, value.to_str().unwrap());
        response = response.header(name, value.to_str().unwrap());
    }
    
    let response_body = response.send().await?.text().await?;
    
    Ok(response_body)
}

async fn make_get_request(url: &str, headers: &HeaderMap) -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();
    let mut response = client.get(url);
    for (name, value) in headers.iter() {
        if name == "Accept-Encoding" {continue;};
        if name == "host" {
            response = response.header("host", "api.app.lovelive-sif2.bushimo.jp");
            continue;
        };
        response = response.header(name, value.to_str().unwrap());
    }
    let response_body = response.send().await?.text().await?;
    Ok(response_body)
}

async fn log_unknown_request(req: HttpRequest, body: String) -> HttpResponse {
    if body != String::new() {
        println!("req: {}", encryption::decrypt_packet(&body).unwrap_or(String::new()));
        let resp = make_post_request(&format!("https://api-sif2.lovelive-sif2.com{}", req.path()), &body, req.headers()).await.unwrap();
        
        //println!("Unhandled request: {} {}", req.path(), body);
        println!("resp: {}", encryption::decrypt_packet(&resp).unwrap_or(String::new()));
        HttpResponse::Ok().body(resp)
    } else {
        let resp = make_get_request(&format!("https://api-sif2.lovelive-sif2.com{}", req.path()), req.headers()).await.unwrap();
        
        //println!("Unhandled request: {} {}", req.path(), body);
        println!("resp: {}", encryption::decrypt_packet(&resp).unwrap_or(String::new()));
        HttpResponse::Ok().body(resp)
        
    }
}*/

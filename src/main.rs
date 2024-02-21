mod encryption;
use actix_web::{
   // post,
   // get,
    HttpResponse,
    HttpRequest,
    http::header::HeaderMap,
    web,
    dev::Service
};
use openssl::ssl::{SslAcceptor, SslFiletype, SslMethod};

async fn make_post_request(url: &str, body: &str, headers: &HeaderMap) -> Result<String, reqwest::Error> {
    let client = reqwest::Client::new();
    let mut response = client
        .post(url)
        .body(body.to_string());
    
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
        let resp = make_post_request(&format!("https://api.app.lovelive-sif2.bushimo.jp{}", req.path()), &body, req.headers()).await.unwrap();
        
        //println!("Unhandled request: {} {}", req.path(), body);
        println!("resp: {}", encryption::decrypt_packet(&resp).unwrap_or(String::new()));
        HttpResponse::Ok().body(resp)
    } else {
        let resp = make_get_request(&format!("https://api.app.lovelive-sif2.bushimo.jp{}", req.path()), req.headers()).await.unwrap();
        
        //println!("Unhandled request: {} {}", req.path(), body);
        println!("resp: {}", encryption::decrypt_packet(&resp).unwrap_or(String::new()));
        HttpResponse::Ok().body(resp)
        
    }
}

#[actix_web::main]
async fn main() -> std::io::Result<()> {
    use actix_web::{App, HttpServer};

    let mut builder = SslAcceptor::mozilla_intermediate(SslMethod::tls()).unwrap();
    builder
        .set_private_key_file("key.pem", SslFiletype::PEM)
        .unwrap();
    builder.set_certificate_chain_file("cert.pem").unwrap();

    let rv = HttpServer::new(|| App::new()
        .wrap_fn(|req, srv| {
            println!("Request: {}", req.path());
            srv.call(req)
        })
        .default_service(web::route().to(log_unknown_request)))
        .bind_openssl("0.0.0.0:8080", builder)?
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
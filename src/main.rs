use axum::{
    extract::Path,
    routing::get,
    routing::post,
    Json, Router,
};
use chrono::Local;
use hyper::{Body, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::net::SocketAddr;
use tokio::fs;
use uuid::Uuid;
async fn serve_image(Path(path): Path<String>) -> Result<Response<Body>, String> {
    if path.find("..").is_some(){
        return Ok(hyper::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap()
            .into());
    }

    let path = format!("uploads{}", path);
    if let Ok(data) = fs::read(&path).await {
        Ok(hyper::Response::new(Body::from(data)))
    } else {
        Ok(hyper::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap()
            .into())
    }
}

#[derive(Debug, Deserialize, Serialize)]
struct UploadResp {
    uri: String,
}

#[derive(Debug, Deserialize, Serialize)]
struct UploadReq {
    base64_img: String,
}

async fn upload(data: String) -> Result<Json<UploadResp>, StatusCode> {
    let data: UploadReq = serde_json::from_str(&data).map_err(|_| StatusCode::BAD_REQUEST)?;
    let base64_img = data.base64_img;
    let suffix = get_img_type(&base64_img).map_err(|_| StatusCode::BAD_REQUEST)?;
    let data = base64::decode(base64_img.split(",").nth(1).unwrap()).map_err(|_| StatusCode::BAD_REQUEST)?;
    let fmt = "%Y-%m-%d";
    let mut now = Local::now().format(fmt).to_string();
    if is_linux() {
        now = now.replace("-", "/");
    } else {
        now = now.replace("-", "\\");
    }
    let base_path = std::path::Path::new("uploads").join(&now);
    println!("{}", base_path.to_str().unwrap());
    if !&base_path.exists() {
        fs::create_dir_all(&base_path).await.unwrap();
    }
    let file_name = format!("{}.{}", Uuid::new_v4(), suffix);
    // let filename = format!("uploads/1.png");
    fs::write(&base_path.join(&file_name), data).await.unwrap();

    let mut uri = String::from("/uploads");
    let path_spilt = "/";
    uri.push_str(&path_spilt);
    uri.push_str(&now.replace("\\", "/"));
    uri.push_str(&path_spilt);
    uri.push_str(&file_name);

    Ok(Json(UploadResp {
        uri: format!(
            "{}",
            uri.to_string()
        ),
    }))
}

fn get_img_type(base64_img: &String) -> Result<String, String> {
    let img_type = base64_img.split(";").nth(0).unwrap().to_string().split(":").nth(1).unwrap().to_string();
    println!("{}", img_type);
    let suffix = img_type.split("/").nth(1).unwrap().to_string();
    let suffixes = vec![
        "png".to_string(),
        "webp".to_string(),
        "jpg".to_string(),
        "jpeg".to_string(),
        "gif".to_string(),
    ];
    if suffixes.contains(&suffix) {
        Ok(suffix)
    } else {
        Err(format!("Unsupported image type: {}", suffix))
    }
}


#[cfg(target_os = "windows")]
fn is_linux() -> bool {
    false
}

#[cfg(target_os = "linux")]
fn is_linux() -> bool {
    true
}

#[tokio::main]
async fn main() {
    // std::env::set_var("RUST_LOG", "trace");
    env_logger::init();
    let app = Router::new()
        .route("/upload", post(upload))
        .route("/uploads/*path", get(serve_image));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

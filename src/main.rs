use async_trait::async_trait;
use axum::{
    body::{Bytes, Empty},
    extract::{FromRequest, Path, RequestParts},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::get,
    routing::post,
    AddExtensionLayer,
    Json, Router,
};
use chrono::Local;
use hyper::{Body, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;

#[derive(Clone)]
struct TokenConfig(Arc<String>);

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

fn validate_token(raw: Option<String>) -> Result<TokenConfig, &'static str> {
    match raw {
        Some(token) if !token.trim().is_empty() => Ok(TokenConfig(Arc::new(token.trim().to_string()))),
        _ => Err("UPLOAD_TOKEN environment variable is not set or empty"),
    }
}

fn load_token() -> Result<TokenConfig, &'static str> {
    validate_token(std::env::var("UPLOAD_TOKEN").ok())
}

struct Unauthorized;

impl IntoResponse for Unauthorized {
    type Body = Empty<Bytes>;
    type BodyError = Infallible;

    fn into_response(self) -> Response<Self::Body> {
        let mut headers = HeaderMap::new();
        headers.insert(header::WWW_AUTHENTICATE, HeaderValue::from_static("Bearer"));
        (StatusCode::UNAUTHORIZED, headers, ()).into_response()
    }
}

struct UploadAuth;

#[async_trait]
impl<B> FromRequest<B> for UploadAuth
where
    B: Send,
{
    type Rejection = Unauthorized;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        let expected = req
            .extensions()
            .and_then(|ext| ext.get::<TokenConfig>())
            .ok_or(Unauthorized)?
            .0
            .clone();

        let provided = req
            .headers()
            .and_then(|headers| headers.get(header::AUTHORIZATION))
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.strip_prefix("Bearer "))
            .ok_or(Unauthorized)?;

        if constant_time_eq(provided.as_bytes(), expected.as_bytes()) {
            Ok(UploadAuth)
        } else {
            Err(Unauthorized)
        }
    }
}

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

async fn upload(_auth: UploadAuth, data: String) -> Result<Json<UploadResp>, StatusCode> {
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
    env_logger::init();

    let token = match load_token() {
        Ok(token) => token,
        Err(e) => {
            eprintln!("{}", e);
            std::process::exit(1);
        }
    };

    let app = Router::new()
        .route("/upload", post(upload))
        .route("/uploads/*path", get(serve_image))
        .layer(AddExtensionLayer::new(token));

    let addr = SocketAddr::from(([0, 0, 0, 0], 3000));
    println!("Listening on http://{}", addr);
    axum::Server::bind(&addr)
        .serve(app.into_make_service())
        .await
        .unwrap();
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::Request;
    use tower::ServiceExt;

    #[test]
    fn constant_time_eq_matches_equal() {
        assert!(constant_time_eq(b"secret", b"secret"));
    }

    #[test]
    fn constant_time_eq_rejects_different_same_len() {
        assert!(!constant_time_eq(b"secret", b"secreT"));
    }

    #[test]
    fn constant_time_eq_rejects_different_len() {
        assert!(!constant_time_eq(b"secret", b"secre"));
    }

    #[test]
    fn constant_time_eq_matches_empty() {
        assert!(constant_time_eq(b"", b""));
    }

    #[test]
    fn validate_token_accepts_non_empty() {
        let config = validate_token(Some("abc123".to_string())).unwrap();
        assert_eq!(*config.0, "abc123".to_string());
    }

    #[test]
    fn validate_token_trims_surrounding_whitespace() {
        let config = validate_token(Some("  secret  ".to_string())).unwrap();
        assert_eq!(*config.0, "secret".to_string());
    }

    #[test]
    fn validate_token_rejects_none() {
        assert!(validate_token(None).is_err());
    }

    #[test]
    fn validate_token_rejects_empty() {
        assert!(validate_token(Some(String::new())).is_err());
    }

    #[test]
    fn validate_token_rejects_whitespace_only() {
        assert!(validate_token(Some("   ".to_string())).is_err());
    }

    fn request_with_auth(value: Option<&str>) -> RequestParts<Body> {
        let mut builder = Request::builder().method("POST").uri("/upload");
        if let Some(v) = value {
            builder = builder.header(header::AUTHORIZATION, v);
        }
        let mut req = builder.body(Body::empty()).unwrap();
        req.extensions_mut()
            .insert(TokenConfig(Arc::new("secret".to_string())));
        RequestParts::new(req)
    }

    #[tokio::test]
    async fn extractor_accepts_valid_token() {
        let mut parts = request_with_auth(Some("Bearer secret"));
        assert!(UploadAuth::from_request(&mut parts).await.is_ok());
    }

    #[tokio::test]
    async fn extractor_rejects_missing_header() {
        let mut parts = request_with_auth(None);
        assert!(UploadAuth::from_request(&mut parts).await.is_err());
    }

    #[tokio::test]
    async fn extractor_rejects_wrong_token() {
        let mut parts = request_with_auth(Some("Bearer wrong"));
        assert!(UploadAuth::from_request(&mut parts).await.is_err());
    }

    #[tokio::test]
    async fn extractor_rejects_non_bearer_scheme() {
        let mut parts = request_with_auth(Some("Basic secret"));
        assert!(UploadAuth::from_request(&mut parts).await.is_err());
    }

    #[tokio::test]
    async fn extractor_rejects_bare_token_without_scheme() {
        let mut parts = request_with_auth(Some("secret"));
        assert!(UploadAuth::from_request(&mut parts).await.is_err());
    }

    #[tokio::test]
    async fn extractor_rejects_empty_token() {
        let mut parts = request_with_auth(Some("Bearer "));
        assert!(UploadAuth::from_request(&mut parts).await.is_err());
    }

    fn app_with_token(token: &str) -> Router {
        Router::new()
            .route("/upload", post(upload))
            .route("/uploads/*path", get(serve_image))
            .layer(AddExtensionLayer::new(TokenConfig(Arc::new(token.to_string()))))
    }

    #[tokio::test]
    async fn upload_route_rejects_missing_token() {
        let request = Request::builder()
            .method("POST")
            .uri("/upload")
            .header("content-type", "application/json")
            .body(Body::from(r#"{"base64_img":"data:image/png;base64,AAAA"}"#))
            .unwrap();

        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer"
        );
    }

    #[tokio::test]
    async fn upload_route_rejects_wrong_token() {
        let request = Request::builder()
            .method("POST")
            .uri("/upload")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, "Bearer wrong")
            .body(Body::from(r#"{"base64_img":"data:image/png;base64,AAAA"}"#))
            .unwrap();

        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            response.headers().get(header::WWW_AUTHENTICATE).unwrap(),
            "Bearer"
        );
    }

    #[tokio::test]
    async fn upload_route_accepts_valid_token_and_reaches_handler() {
        let request = Request::builder()
            .method("POST")
            .uri("/upload")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(Body::from("not-json"))
            .unwrap();

        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }
}

use async_trait::async_trait;
use axum::{
    body::{Bytes, Empty},
    extract::{FromRequest, Path, RequestParts},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::get,
    routing::post,
    AddExtensionLayer, Json, Router,
};
use chrono::Local;
use hyper::{body::HttpBody, Body, Response, StatusCode};
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
        Some(token) if !token.trim().is_empty() => {
            Ok(TokenConfig(Arc::new(token.trim().to_string())))
        }
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
    if path.find("..").is_some() {
        return Ok(hyper::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap());
    }

    let path = format!("uploads{}", path);
    if let Ok(data) = fs::read(&path).await {
        Ok(hyper::Response::new(Body::from(data)))
    } else {
        Ok(hyper::Response::builder()
            .status(StatusCode::NOT_FOUND)
            .body(Body::from("Not Found"))
            .unwrap())
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

/// 单次请求的 JSON body 上限（含 base64 编码开销）。
const MAX_UPLOAD_BODY_BYTES: usize = 10 * 1024 * 1024;
/// 解码后的图片大小上限。
const MAX_IMAGE_BYTES: usize = 7 * 1024 * 1024;

/// 带上限的请求体：先看 `Content-Length`，再按实际读取量兜底，超限返回 413。
struct UploadBody(Vec<u8>);

#[async_trait]
impl FromRequest<Body> for UploadBody {
    type Rejection = StatusCode;

    async fn from_request(req: &mut RequestParts<Body>) -> Result<Self, Self::Rejection> {
        if let Some(length) = req
            .headers()
            .and_then(|headers| headers.get(header::CONTENT_LENGTH))
            .and_then(|value| value.to_str().ok())
            .and_then(|value| value.parse::<u64>().ok())
        {
            if length > MAX_UPLOAD_BODY_BYTES as u64 {
                return Err(StatusCode::PAYLOAD_TOO_LARGE);
            }
        }

        let body = req.take_body().ok_or(StatusCode::BAD_REQUEST)?;
        Ok(Self(read_limited(body, MAX_UPLOAD_BODY_BYTES).await?))
    }
}

async fn read_limited(mut body: Body, max: usize) -> Result<Vec<u8>, StatusCode> {
    let mut data = Vec::new();
    while let Some(chunk) = body.data().await {
        let chunk = chunk.map_err(|_| StatusCode::BAD_REQUEST)?;
        if data.len() + chunk.len() > max {
            return Err(StatusCode::PAYLOAD_TOO_LARGE);
        }
        data.extend_from_slice(&chunk);
    }
    Ok(data)
}

fn decode_image(payload: &str, max: usize) -> Result<Vec<u8>, StatusCode> {
    let data = base64::decode(payload).map_err(|_| StatusCode::BAD_REQUEST)?;
    if data.len() > max {
        return Err(StatusCode::PAYLOAD_TOO_LARGE);
    }
    Ok(data)
}

async fn upload(
    _auth: UploadAuth,
    UploadBody(body): UploadBody,
) -> Result<Json<UploadResp>, StatusCode> {
    let data: UploadReq = serde_json::from_slice(&body).map_err(|_| StatusCode::BAD_REQUEST)?;
    let base64_img = data.base64_img;
    let suffix = get_img_type(&base64_img).map_err(|_| StatusCode::BAD_REQUEST)?;
    let (_, payload) = base64_img.split_once(',').ok_or(StatusCode::BAD_REQUEST)?;
    let data = decode_image(payload, MAX_IMAGE_BYTES)?;
    let fmt = "%Y-%m-%d";
    let mut now = Local::now().format(fmt).to_string();
    if is_linux() {
        now = now.replace("-", "/");
    } else {
        now = now.replace("-", "\\");
    }
    let base_path = std::path::Path::new("uploads").join(&now);
    fs::create_dir_all(&base_path)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;
    let file_name = format!("{}.{}", Uuid::new_v4(), suffix);
    fs::write(&base_path.join(&file_name), data)
        .await
        .map_err(|_| StatusCode::INTERNAL_SERVER_ERROR)?;

    let mut uri = String::from("/uploads");
    let path_spilt = "/";
    uri.push_str(path_spilt);
    uri.push_str(&now.replace("\\", "/"));
    uri.push_str(path_spilt);
    uri.push_str(&file_name);

    Ok(Json(UploadResp { uri }))
}

fn get_img_type(base64_img: &str) -> Result<String, String> {
    let img_type = base64_img
        .split(';')
        .next()
        .and_then(|value| value.split(':').nth(1))
        .ok_or_else(|| "invalid data URL".to_string())?;
    let suffix = img_type
        .split('/')
        .nth(1)
        .ok_or_else(|| "invalid data URL".to_string())?;
    if ["png", "webp", "jpg", "jpeg", "gif"].contains(&suffix) {
        Ok(suffix.to_string())
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
            .layer(AddExtensionLayer::new(TokenConfig(Arc::new(
                token.to_string(),
            ))))
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

    #[tokio::test]
    async fn uploads_route_stays_public_without_token() {
        let request = Request::builder()
            .method("GET")
            .uri("/uploads/nope.png")
            .body(Body::empty())
            .unwrap();

        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::NOT_FOUND);
    }

    #[test]
    fn get_img_type_accepts_supported_types() {
        for mime in ["png", "webp", "jpg", "jpeg", "gif"] {
            let input = format!("data:image/{};base64,AAAA", mime);
            assert_eq!(get_img_type(&input).unwrap(), mime);
        }
    }

    #[test]
    fn get_img_type_rejects_unsupported_type() {
        assert!(get_img_type("data:image/bmp;base64,AAAA").is_err());
    }

    #[test]
    fn get_img_type_rejects_malformed_data_url() {
        assert!(get_img_type("not-a-data-url").is_err());
        assert!(get_img_type("data:image;base64,AAAA").is_err());
    }

    fn upload_request(body: &'static str) -> Request<Body> {
        Request::builder()
            .method("POST")
            .uri("/upload")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, "Bearer secret")
            .body(Body::from(body))
            .unwrap()
    }

    #[tokio::test]
    async fn upload_route_rejects_missing_comma() {
        let request = upload_request(r#"{"base64_img":"data:image/png;base64"}"#);
        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upload_route_rejects_unsupported_type() {
        let request = upload_request(r#"{"base64_img":"data:image/bmp;base64,AAAA"}"#);
        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upload_route_rejects_invalid_base64() {
        let request = upload_request(r#"{"base64_img":"data:image/png;base64,@@@@"}"#);
        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::BAD_REQUEST);
    }

    #[tokio::test]
    async fn upload_route_rejects_oversized_content_length() {
        let request = Request::builder()
            .method("POST")
            .uri("/upload")
            .header("content-type", "application/json")
            .header(header::AUTHORIZATION, "Bearer secret")
            .header(
                header::CONTENT_LENGTH,
                (MAX_UPLOAD_BODY_BYTES + 1).to_string(),
            )
            .body(Body::empty())
            .unwrap();

        let response = app_with_token("secret").oneshot(request).await.unwrap();
        assert_eq!(response.status(), StatusCode::PAYLOAD_TOO_LARGE);
    }

    #[tokio::test]
    async fn read_limited_rejects_body_over_limit() {
        let body = Body::from(vec![0u8; 32]);
        assert_eq!(
            read_limited(body, 16).await.unwrap_err(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[tokio::test]
    async fn read_limited_accepts_body_within_limit() {
        let body = Body::from(vec![0u8; 8]);
        assert_eq!(read_limited(body, 16).await.unwrap().len(), 8);
    }

    #[test]
    fn decode_image_rejects_image_over_limit() {
        let payload = base64::encode(vec![0u8; 32]);
        assert_eq!(
            decode_image(&payload, 16).unwrap_err(),
            StatusCode::PAYLOAD_TOO_LARGE
        );
    }

    #[test]
    fn decode_image_accepts_image_within_limit() {
        let payload = base64::encode(vec![0u8; 32]);
        assert_eq!(decode_image(&payload, 32).unwrap().len(), 32);
    }
}

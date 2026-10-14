# 上传接口 Token 校验实现计划

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** 为 `POST /upload` 增加基于静态 token 的鉴权，token 来自环境变量 `UPLOAD_TOKEN`，通过 `Authorization: Bearer <token>` 传递。

**Architecture:** 在单文件 `src/main.rs` 中新增一个自定义 axum 0.3 `FromRequest` 提取器 `UploadAuth`，作为上传接口的类型级鉴权守卫。token 由启动时读取的环境变量包装成 `TokenConfig`，经 `AddExtensionLayer` 注入请求扩展供提取器读取；比较使用手写常量时间算法。`GET /uploads/*path` 不受影响。

**Tech Stack:** Rust 2021、axum 0.3.4、tokio、tower 0.4、async-trait 0.1、http（经 axum re-export）。无需新增依赖。

**参考规格:** `docs/superpowers/specs/2026-10-14-upload-token-auth-design.md`

---

## 文件结构

- 修改：`src/main.rs` — 全部鉴权逻辑与测试（项目为单文件结构，测试放在文件内 `#[cfg(test)] mod tests`）。
- 修改：`README.md` — 补充 `UPLOAD_TOKEN` 环境变量说明。
- 不改动：`Cargo.toml`（现有依赖 `axum`、`tower`、`async-trait` 已足够）。

---

## Task 1: 常量时间比较与 token 配置加载

**Files:**
- Modify: `src/main.rs`（在 `use` 区块与 `#[tokio::main] fn main` 之间追加；测试追加到新增的 `#[cfg(test)] mod tests`）

- [ ] **Step 1: 添加 `use` 导入**

将 `src/main.rs` 顶部导入区替换为下面内容（保留原有项，新增标注的行）：

```rust
use axum::{
    body::{Bytes, Empty},
    extract::Path,
    extract::{FromRequest, RequestParts},
    http::{header, HeaderMap, HeaderValue},
    response::IntoResponse,
    routing::get,
    routing::post,
    AddExtensionLayer, Json, Router,
};
use async_trait::async_trait;
use chrono::Local;
use hyper::{Body, Response, StatusCode};
use serde::{Deserialize, Serialize};
use std::convert::Infallible;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::fs;
use uuid::Uuid;
```

- [ ] **Step 2: 写失败测试**

在 `src/main.rs` 末尾追加测试模块。为了让 `validate_token` 可被测试，本任务先只加 `constant_time_eq` 与 `validate_token` 的测试：

```rust
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
}
```

- [ ] **Step 3: 运行测试确认失败**

Run: `cargo test --bin img-server`
Expected: 编译失败，报 `cannot find function 'constant_time_eq'` 与 `cannot find function 'validate_token'`（或 `TokenConfig` 未定义）。

- [ ] **Step 4: 实现最小代码**

在 `src/main.rs` 的 `use` 区块之后、`#[tokio::main]` 之前添加：

```rust
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
        Some(token) if !token.trim().is_empty() => Ok(TokenConfig(Arc::new(token))),
        _ => Err("UPLOAD_TOKEN environment variable is not set or empty"),
    }
}

fn load_token() -> Result<TokenConfig, &'static str> {
    validate_token(std::env::var("UPLOAD_TOKEN").ok())
}
```

- [ ] **Step 5: 运行测试确认通过**

Run: `cargo test --bin img-server`
Expected: 所有测试 PASS（`validate_token_*`、`constant_time_eq_*` 共 8 个），其余既有测试（若有）通过。

- [ ] **Step 6: 提交**

```bash
git add src/main.rs
git commit -m "feat: 增加常量时间比较与 token 配置加载"
```

---

## Task 2: `Unauthorized` 响应与 `UploadAuth` 提取器

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 写失败测试**

在 `mod tests` 内追加下面的辅助函数与用例（放在已有测试之后、模块右花括号之前）：

```rust
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
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --bin img-server extractor_`
Expected: 编译失败，报 `cannot find type 'UploadAuth'`。

- [ ] **Step 3: 实现最小代码**

在 `validate_token` 之后添加：

```rust
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
```

- [ ] **Step 4: 运行测试确认通过**

Run: `cargo test --bin img-server extractor_`
Expected: 6 个 `extractor_*` 测试全部 PASS。

- [ ] **Step 5: 提交**

```bash
git add src/main.rs
git commit -m "feat: 增加 UploadAuth 提取器与 401 响应"
```

---

## Task 3: 接入路由并 fail-fast 加载 token

**Files:**
- Modify: `src/main.rs`

- [ ] **Step 1: 写失败测试**

在 `mod tests` 内追加路由级集成测试（鉴权失败路径不触碰文件系统）：

```rust
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
    }
```

- [ ] **Step 2: 运行测试确认失败**

Run: `cargo test --bin img-server upload_route_`
Expected: 编译失败，报 `this function takes 2 arguments but 1 was supplied`（`upload` 尚未接收 `UploadAuth`），或未定义相关类型。

- [ ] **Step 3: 修改 `upload` handler 签名**

将现有：

```rust
async fn upload(data: String) -> Result<Json<UploadResp>, StatusCode> {
```

改为：

```rust
async fn upload(_auth: UploadAuth, data: String) -> Result<Json<UploadResp>, StatusCode> {
```

handler 其余函数体保持不变。

- [ ] **Step 4: 修改 `main` 接线**

将现有 `main` 函数替换为：

```rust
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
```

- [ ] **Step 5: 运行全部测试确认通过**

Run: `cargo test --bin img-server`
Expected: 全部 PASS（含 `upload_route_rejects_missing_token`、`upload_route_rejects_wrong_token`）。

- [ ] **Step 6: 运行编译与 lint 检查**

Run: `cargo build --bin img-server`
Expected: 编译成功，无 error。

Run: `cargo clippy --bin img-server -- -D warnings`（若环境未安装 clippy，可跳过并记录）
Expected: 无 warning/error。

- [ ] **Step 7: 提交**

```bash
git add src/main.rs
git commit -m "feat: 上传接口接入 token 校验并 fail-fast 加载配置"
```

---

## Task 4: 更新 README 并最终验证

**Files:**
- Modify: `README.md`

- [ ] **Step 1: 更新 README**

将 `README.md` 的 docker 部署示例更新为包含 `UPLOAD_TOKEN`：

```markdown
# rust-img-server


## 环境变量

- `UPLOAD_TOKEN`：上传接口鉴权 token。服务启动时必须提供，否则启动失败。
  客户端调用 `POST /upload` 时需携带请求头 `Authorization: Bearer <token>`。


## docker部署

```angular2html
docker run -d -u 1001 --name img-server \
    -e UPLOAD_TOKEN=your-secret-token \
    -v ${PWD}/uploads:/opt/work/uploads \
    -p 3000:3000 \
    pfxuresource/img-server:centos7_v0.1.0


```
```

- [ ] **Step 2: 验证 fail-fast 行为**

Run: `cargo run --bin img-server`（不设置 `UPLOAD_TOKEN`）
Expected: 打印 `UPLOAD_TOKEN environment variable is not set or empty`，进程以退出码 1 结束（不会打印 `Listening on ...`）。

- [ ] **Step 3: 验证启动与鉴权端到端行为**

先启动：

```bash
UPLOAD_TOKEN=secret cargo run --bin img-server
```

另开一个终端，验证鉴权失败（预期 401）：

```bash
curl -s -o /dev/null -w "%{http_code}\n" -X POST http://127.0.0.1:3000/upload \
  -H "content-type: application/json" \
  -d '{"base64_img":"data:image/png;base64,iVBORw0KGgo="}'
```

Expected: `401`

验证鉴权通过（预期 200 且返回 `uri`）：

```bash
curl -s -X POST http://127.0.0.1:3000/upload \
  -H "authorization: Bearer secret" \
  -H "content-type: application/json" \
  -d '{"base64_img":"data:image/png;base64,iVBORw0KGgo="}'
```

Expected: 返回 `{"uri":"/uploads/..."}`。验证完成后停止服务（Ctrl-C）。

- [ ] **Step 4: 提交**

```bash
git add README.md
git commit -m "docs: 说明 UPLOAD_TOKEN 环境变量与鉴权用法"
```

---

## 自审结果

- **规格覆盖：** 静态 token（Task 1）、`Authorization: Bearer`（Task 2）、仅保护 upload（Task 3 路由结构）、`UPLOAD_TOKEN` 环境变量与 fail-fast（Task 1 + Task 3）、`GET /uploads/*path` 公开（Task 3，仅对全路由加 `AddExtensionLayer`，无鉴权层）、常量时间比较（Task 1）、错误统一 401（Task 2）、测试策略（Task 1/2/3 单测与集成测试）均有对应任务。
- **占位符扫描：** 无 TBD/TODO，所有代码步骤含完整代码。
- **类型一致性：** `TokenConfig(Arc<String>)`、`UploadAuth`、`Unauthorized`、`constant_time_eq`、`validate_token`、`load_token` 在各任务中命名与签名一致；`upload` 签名在 Task 3 明确定义，测试与 handler 一致。

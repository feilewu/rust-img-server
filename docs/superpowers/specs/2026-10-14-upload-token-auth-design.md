# 上传接口 Token 校验设计

日期：2026-10-14
状态：已批准

## 背景

`rust-img-server` 是一个单文件（`src/main.rs`）的 axum 0.3 图片服务：

- `POST /upload`：接收 JSON `{ "base64_img": "data:image/png;base64,..." }`，解码后写入 `uploads/<日期>/<uuid>.<ext>`，返回 `{ "uri": "/uploads/..." }`。
- `GET /uploads/*path`：读取并返回图片。

当前两个接口均无鉴权，任何人都可以上传，存在被滥用的风险。本设计为上传接口增加静态 token 校验。

## 需求

- 校验方式：固定密钥（静态 token）。
- 传递方式：请求头 `Authorization: Bearer <token>`。
- 保护范围：仅 `POST /upload`；`GET /uploads/*path` 保持公开可访问。
- 配置来源：环境变量 `UPLOAD_TOKEN`。
- 未配置行为：启动失败（打印错误并 `exit(1)`），避免服务在未设防状态下运行。

## 方案选择

在三个候选方案中（`tower-http` 的 `RequireAuthorizationLayer`、自定义 `FromRequest` 提取器、handler 内联校验），选择**自定义 `FromRequest` 提取器**：

- 不引入新依赖（仅使用现有 `axum` / `http` 能力）。
- 类型安全：只有鉴权通过的请求才能拿到 `UploadAuth` 并进入上传逻辑。
- 边界清晰、可独立单测，不把鉴权逻辑混进业务 handler。

## 组件设计

全部改动位于 `src/main.rs`。

### 1. `TokenConfig`

```rust
#[derive(Clone)]
struct TokenConfig(Arc<String>);
```

- 启动时从 `UPLOAD_TOKEN` 读取并包装。
- 通过 `AddExtensionLayer` 注入请求扩展，供提取器读取。
- `Clone + Send + Sync + 'static`，满足 layer 与提取器要求。

### 2. `UploadAuth` 提取器

零大小结构体，实现 `FromRequest`：

```rust
struct UploadAuth;

#[async_trait]
impl<B> FromRequest<B> for UploadAuth
where
    B: Send,
{
    type Rejection = Unauthorized;

    async fn from_request(req: &mut RequestParts<B>) -> Result<Self, Self::Rejection> {
        // 1. 借用 req.headers() 读取 "authorization"
        // 2. 要求形如 "Bearer <token>"（scheme 精确匹配）
        // 3. 从 req.extensions() 取 TokenConfig
        // 4. constant_time_eq 比较
        // 任一环节失败 -> Err(Unauthorized)
    }
}
```

要点：

- 使用 `req.headers()`（借用）而非 `take_headers()`，保证后续 `String` 提取器仍能读取 body。
- 从 `req.extensions()` 读取 `TokenConfig`。
- 比较使用常量时间算法。
- scheme 匹配为大小写敏感的精确匹配 `Bearer `（含一个空格）；token 部分按原样取出，不做 trim（避免误放宽），但空白 token 视为失败。

### 3. `Unauthorized` rejection

```rust
struct Unauthorized;

impl IntoResponse for Unauthorized {
    type Body = axum::body::Empty<Bytes>;
    type BodyError = std::convert::Infallible;

    fn into_response(self) -> Response<Self::Body> {
        // 401 + WWW-Authenticate: Bearer
    }
}
```

注意 axum 0.3 的 `IntoResponse` 采用关联类型（`Body` / `BodyError`）签名，而非后续版本的 `fn into_response(self) -> Response<Body>`。实现时借助已有的 `(StatusCode, T)` 组合 impl 构造响应。

用于提取失败时统一返回 `401 Unauthorized`。

### 4. `constant_time_eq`

```rust
fn constant_time_eq(a: &[u8], b: &[u8]) -> bool;
```

手写实现：长度不等直接返回 `false`（长度本身非敏感），长度相等时对全部字节做 XOR 累加，最终判断是否为 0。避免计时侧信道，不引入新依赖。

### 5. `load_token`

```rust
fn load_token() -> Result<TokenConfig, &'static str>;
```

读取 `UPLOAD_TOKEN`；缺失或去除空白后为空则返回 `Err`。`main` 中 `match` 处理错误：打印到 stderr 并 `std::process::exit(1)`。

### 6. 路由与 `main`

```rust
let token = match load_token() { ... };

let app = Router::new()
    .route("/upload", post(upload))
    .route("/uploads/*path", get(serve_image))
    .layer(AddExtensionLayer::new(token));

// 绑定并启动（保持现状 0.0.0.0:3000）
```

上传 handler 签名改为：

```rust
async fn upload(_auth: UploadAuth, data: String) -> Result<Json<UploadResp>, StatusCode>;
```

## 数据流

1. 客户端 `POST /upload`，携带 `Authorization: Bearer <token>` 与 JSON body。
2. axum 先执行 `UploadAuth::from_request`：
   - 借用读取 `authorization` 头。
   - 精确匹配 `Bearer ` 前缀并取出 token 部分。
   - 从扩展取 `TokenConfig`。
   - 常量时间比较。
3. 成功 → 继续执行 `upload`，走现有上传逻辑。
4. 失败 → 返回 `Unauthorized`（401），异常请求不会进入上传逻辑。
5. `GET /uploads/*path` 不挂任何鉴权 layer，公开读取。

## 错误处理

- 缺失 `Authorization` 头、scheme 非 `Bearer`、token 为空、token 不匹配：统一返回 `401 Unauthorized`，不区分具体原因，避免信息泄露。
- 响应附带 `WWW-Authenticate: Bearer`。
- 启动时 `UPLOAD_TOKEN` 缺失或为空白：打印错误到 stderr 并退出码 1。

## 测试策略

仅测试鉴权相关逻辑；不改变既有上传行为。

1. **提取器单测**（使用 `RequestParts::new` 构造请求，不触碰文件系统）：
   - 有效 token → `Ok`。
   - 缺失 `Authorization` 头 → `Err`。
   - 错误 token → `Err`。
   - scheme 非 `Bearer`（如 `Basic`、裸 token）→ `Err`。
   - 空 token / `Bearer ` 后空白 → `Err`。
2. **`constant_time_eq` 单测**：相等、不等（等长）、长度不同。
3. **路由级集成测试**（`tower::ServiceExt::oneshot`）：
   - 无 token → 401。
   - 错误 token → 401。
   - 以上用例均不写入文件系统。

## 范围之外（YAGNI）

- 不做 JWT / 签名 / 多用户 / 权限分级。
- 不做请求限流、token 轮换、审计日志。
- 不保护 `GET /uploads/*path`。
- 除启动时读取 `UPLOAD_TOKEN` 外不做额外配置系统。

## 部署影响

- 启动容器时需提供 `UPLOAD_TOKEN` 环境变量，例如：
  `docker run ... -e UPLOAD_TOKEN=<secret> ...`
- README 需补充该环境变量说明（实现阶段处理）。

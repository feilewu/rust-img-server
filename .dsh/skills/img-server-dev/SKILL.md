---
name: img-server-dev
description: Build, run, test, and smoke-test rust-img-server — a single-file Rust/axum 0.3 image server. Use when changing src/main.rs, running cargo commands, or verifying /upload and /uploads behaviour.
---

# rust-img-server 开发

## 工程形态

- 单 binary：`src/main.rs` 一个文件，Rust 2021，测试写在文件内 `#[cfg(test)] mod tests`。
- 依赖刻意偏旧：`axum 0.3`、`hyper 0.14`、`base64 0.13`、`uuid 0.8`、`tokio 1`。
- 工具链由 `rust-toolchain.toml` 固定（当前 1.99.0），CI 用 rustup 按它安装。
- 启动即要求 `UPLOAD_TOKEN`，缺失/空白直接 stderr 报错并 `exit(1)`。

## 常用命令

```bash
cargo build
cargo test --bin img-server                    # 项目约定的测试入口（基线 19 个用例全绿）
cargo clippy --bin img-server -- -D warnings   # 已清零，作为硬性门禁
cargo fmt --check                              # 已通过
cargo build --release
```

> 基线现状（2026-10-18，Rust 1.99.0）：19 tests、`cargo fmt --check`、`clippy -D warnings` 全绿。
> 提交前这三项都应通过；不要再退回「基线本来就不干净」的旧状态。

本地运行（必须在仓库根目录，`uploads/` 相对当前目录生成）：

```bash
UPLOAD_TOKEN=secret cargo run --bin img-server
# 监听 0.0.0.0:3000
```

## 接口契约

- `POST /upload`，`Authorization: Bearer <UPLOAD_TOKEN>`，JSON：
  `{"base64_img":"data:image/png;base64,..."}`
  支持 png / webp / jpg / jpeg / gif；写 `uploads/YYYY/MM/DD/<uuid>.<ext>`，返回 `{"uri":"/uploads/..."}`。
- `GET /uploads/*path` 公开、无鉴权，从工作目录下的 `uploads/` 读取。
- 失败语义：未带/错 token → `401`（带 `WWW-Authenticate: Bearer`）；JSON 非法或类型不支持 → `400`。

冒烟测试：

```bash
curl -s -o /dev/null -w '%{http_code}\n' -X POST http://127.0.0.1:3000/upload \
  -H 'content-type: application/json' \
  -d '{"base64_img":"data:image/png;base64,iVBORw0KGgo="}'        # 期望 401

curl -s -X POST http://127.0.0.1:3000/upload \
  -H 'authorization: Bearer secret' -H 'content-type: application/json' \
  -d '{"base64_img":"data:image/png;base64,iVBORw0KGgo="}'        # 期望 {"uri":"/uploads/..."}
```

## 改动时注意

- 这是 axum **0.3** API（`FromRequest` + `RequestParts`、`AddExtensionLayer`、`hyper::Body`）。不要照搬 0.6+ 的 `State`/`axum::extract::Request` 写法。
- 小功能优先不加新依赖；鉴权就是手写 `FromRequest` 提取器的先例。
- 请求路径上不允许 `unwrap/expect`：畸形 data URL、base64 解码失败返回 `400`，建目录/写文件失败返回 `500`，并已有对应测试。
- 日期目录用服务器本地时区（`Local::now()`），Linux 下把 `-` 换成 `/`；`is_linux()` 只有 windows/linux 两个 cfg，macOS 上会编译失败。
- `uploads/` 已在 `.gitignore`，不要提交测试产物。

## 收尾验证

1. `cargo test --bin img-server` 全绿；
2. `cargo clippy --bin img-server -- -D warnings` 无输出；
3. `cargo fmt --check` 通过；
4. `cargo build`（或 `--release`，看是否动到部署）；
5. 改了路由/鉴权就跑上面的 curl 冒烟；
6. 需要出镜像时转 `img-server-release` skill。

CI（`.github/workflows/ci.yml`）在 push 时会在 amd64、arm64 两个 runner 上跑 1–5，push 后可在 Actions 里确认。

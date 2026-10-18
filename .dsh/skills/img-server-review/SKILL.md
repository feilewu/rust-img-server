---
name: img-server-review
description: Review checklist for rust-img-server changes — auth boundary, path traversal, panic/error mapping, upload limits, tests, and deployment consistency.
---

# rust-img-server 评审清单

按下面顺序过一遍，只报实际命中的问题，并给出文件:行号。

## 1. 鉴权边界

- `POST /upload` 是否仍由 `UploadAuth` 提取器保护？新增的写接口是否也挂了鉴权？
- token 比较是否保持常量时间；日志、错误响应、返回体里不得出现 token。
- `GET /uploads/*path` 设计上公开，不要误加鉴权；但也别把敏感文件放进 `uploads/`。
- `UPLOAD_TOKEN` 仍是启动 fail-fast 吗？空白 token 应拒绝。

## 2. 路径与文件

- `serve_image` 目前用 `path.find("..")` 拦截穿越；注意 URL 解码后的形态、以及 `uploads` 前缀拼接是否可被绝对路径绕过。更稳的是 canonicalize 后校验前缀，并考虑符号链接。
- 读取响应没有设置 `Content-Type`，浏览器靠嗅探；按扩展名补 MIME 是合理改进。

## 3. panic 与错误映射

- 现状已修：`upload` 对缺失逗号、非法 base64、类型不支持返回 `400`，`create_dir_all` / `fs::write` 失败返回 `500`，请求路径上无 `unwrap`。
- 新增代码继续保持：请求路径上禁止 `unwrap/expect`，用 `map_err` 映射状态码，并补对应的 4xx 路由测试。

## 4. 资源与滥用

- 已实现：请求体 ≤ 10 MiB（`Content-Length` + 流式计数双保险）、解码后图片 ≤ 7 MiB、上传并发 ≤ 8，超限分别返回 `413` / `429`。
- 仍缺：per-IP 速率限制和磁盘配额；公开部署建议在反向代理层补速率限制。
- base64 解码仍整块进内存，但已被 body 上限和并发上限共同约束。

## 5. 测试

- 鉴权相关测试不得触碰文件系统；路由级用 `tower::ServiceExt::oneshot`。
- 测试放在 `src/main.rs` 的 `#[cfg(test)] mod tests`，入口 `cargo test --bin img-server`。
- 修 bug 时先补失败用例；改了 401 行为要覆盖 `WWW-Authenticate`。

## 6. 部署一致性

- 版本号已对齐：`Cargo.toml` 1.1.0 = 最近 tag `v1.1.0` = README 镜像示例；发版时三者同步改。
- 部署以 `DockerFile` 为准：容器内 uid `1000`，README 不再用 `-u` 覆盖；`uploads` 卷需 uid 1000 可写。
- 新依赖是否会破坏旧工具链（`rust-toolchain.toml` 固定版本）或 CI（ubuntu-22.04）。

## 7. 约定

- 保持单文件结构，除非明确要拆模块。
- 提交信息沿用 `feat:` / `test:` / `docs:`（中文描述）。
- 非平凡改动先写 `docs/superpowers/specs/` 设计，再写 `docs/superpowers/plans/` 计划，按 TDD 步骤执行。

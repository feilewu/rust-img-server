---
name: img-server-release
description: Release rust-img-server — tag-driven GitHub Actions build and Docker Hub push, plus local image build/run and version-consistency checks.
---

# rust-img-server 发布

## 发布机制

- 触发：推送任意 tag（workflow 里是 `tags: '*'`），例如 `git push origin v1.2.0`。
- 流程（`.github/workflows/build-and-push-image-ubuntu.yml`）：
  1. ubuntu-22.04 上 rustup 安装 `rust-toolchain.toml` 指定的工具链，`cargo build --release`；
  2. 上传 `target/release/img-server` 作为 artifact；
  3. 下载 binary 后 `docker/build-push-action` 构建并推送 **`pfxuresources/img-server:<tag>`**。
- 依赖 secrets：`DOCKERHUB_USERNAME`、`DOCKERHUB_TOKEN`。
- 镜像 tag 取自 git tag 名本身，**不读** `Cargo.toml` 的 version。

## 发布步骤

1. 确认 `cargo test --bin img-server` 通过、工作区干净。
2. 把 `Cargo.toml` 的 `version` 改成与即将打的 tag 一致（当前 1.1.0，与镜像 tag `v1.1.0` 对齐）。
3. 需要时同步 README 里的 `docker run` 示例 tag。
4. 提交后打 tag 并推送：

```bash
git tag -a v1.2.0 -m "v1.2.0"
git push origin v1.2.0
```

5. 在 Actions 里确认两个 job（build-img-server → build-image-and-push）成功，再验证镜像：

```bash
docker pull pfxuresources/img-server:v1.2.0
```

## 本地构建镜像

Dockerfile **不编译**，只 `COPY target/release/img-server`，所以必须先出二进制：

```bash
cargo build --release
docker build -f DockerFile -t pfxuresources/img-server:v1.2.0 .
docker run -d --name img-server \
  -e UPLOAD_TOKEN=secret \
  -v "$PWD/uploads:/opt/work/uploads" \
  -p 3000:3000 \
  pfxuresources/img-server:v1.2.0
```

## 已知坑

- 部署以 `DockerFile` 为准：容器内 `user 1000`，README 示例不再用 `-u` 覆盖；挂载前需 `chown -R 1000:1000 uploads`。
- 镜像 tag 与 git tag 同名，README 示例已同步为 `v1.1.0`；`Cargo.toml` 版本也保持一致。
- 旧 tag 命名（`centos7_v0.1.0`、`ubuntu_v1.0.0`）来自已 `.disabled` 的 workflow，不要再用。
- `rust-toolchain.toml` 改动会直接影响 CI 构建环境，非必要不动。

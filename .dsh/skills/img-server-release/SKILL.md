---
name: img-server-release
description: Release rust-img-server — tag-driven GitHub Actions build and Docker Hub push, plus local image build/run and version-consistency checks.
---

# rust-img-server 发布

## 发布机制

- 触发：推送任意 tag（workflow 里是 `tags: '*'`），例如 `git push origin v1.2.0`；也可在 Actions 页面手动 `workflow_dispatch`。
- 流程（`.github/workflows/build-and-push-image-ubuntu.yml`），两个原生构建 + 一个合并：
  1. `build-push-amd64`：ubuntu-22.04 上 `cargo build --release`，构建并推送 **`<tag>-amd64`**；
  2. `build-push-arm64`：ubuntu-22.04-arm 上原生构建，推送 **`<tag>-arm64`**；
  3. `manifest`：用 `docker buildx imagetools create` 把两个 tag 合成多架构 **`<tag>`**。
- 依赖 secrets：`DOCKERHUB_USERNAME`、`DOCKERHUB_TOKEN`。
- 镜像 tag 取自 git tag 名（手动触发时取分支名），**不读** `Cargo.toml` 的 version。
- README 里的镜像示例指向 `v1.1.0`（单架构）；下一次发版后应改成新的多架构 tag。

## 发布步骤

1. 确认 `cargo test --bin img-server` 通过、工作区干净。
2. 把 `Cargo.toml` 的 `version` 改成与即将打的 tag 一致（当前 1.1.0，与镜像 tag `v1.1.0` 对齐）。
3. 需要时同步 README 里的 `docker run` 示例 tag。
4. 提交后打 tag 并推送：

```bash
git tag -a v1.2.0 -m "v1.2.0"
git push origin v1.2.0
```

5. 在 Actions 里确认三个 job（`build-push-amd64`、`build-push-arm64`、`manifest`）成功，再验证镜像与架构：

```bash
docker pull pfxuresources/img-server:v1.2.0
docker buildx imagetools inspect pfxuresources/img-server:v1.2.0
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
- `v1.1.0` 及更早镜像是单架构 amd64；ARM 上拉旧 tag 需 `--platform linux/amd64` 或按 README 本地构建。
- arm64 job 依赖公共仓库的 `ubuntu-22.04-arm` runner（免费）；私有仓库/组织需要 larger runner。
- 旧 tag 命名（`centos7_v0.1.0`、`ubuntu_v1.0.0`）来自已 `.disabled` 的 workflow，不要再用。
- `rust-toolchain.toml` 改动会直接影响 CI 构建环境，非必要不动。

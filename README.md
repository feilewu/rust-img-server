# rust-img-server


## 环境变量

- `UPLOAD_TOKEN`：上传接口鉴权 token。服务启动时必须提供，否则启动失败。
  客户端调用 `POST /upload` 时需携带请求头 `Authorization: Bearer <token>`；
  token 缺失或不正确时返回 `401 Unauthorized`。
- 读取接口 `GET /uploads/*path` 无需鉴权，可直接访问。


## docker 部署

镜像内以 uid `1000` 运行（见 `DockerFile` 的 `user 1000`）。挂载的 `uploads` 目录需要 uid 1000 可写：

```bash
mkdir -p uploads
sudo chown -R 1000:1000 uploads

docker run -d --name img-server \
    -e UPLOAD_TOKEN=your-secret-token \
    -v ${PWD}/uploads:/opt/work/uploads \
    -p 3000:3000 \
    pfxuresources/img-server:v1.1.0
```

- 不要用 `-u` 覆盖容器用户，否则与 `DockerFile` 的 uid `1000` 不一致，挂载目录会写不进去。
- 镜像 tag 与 git tag 同名，由 CI（`.github/workflows/build-and-push-image-ubuntu.yml`）在推送 tag 时构建并推送到 `pfxuresources/img-server`。

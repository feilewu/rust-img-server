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
    pfxuresources/img-server:centos7_v0.1.0


```

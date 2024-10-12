# rust-img-server


## docker部署

```angular2html
docker run -d -u 1001 --name img-server \
    -v ${PWD}/uploads:/opt/work/uploads \
    -p 3000:3000 \
    pfxuresources/img-server:centos7_v0.1.0


```
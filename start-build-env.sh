user=$(whoami)

docker build -t img-build-ubuntu:"${user}" -f- . <<EOF
FROM ubuntu:20.04
RUN apt-get update -y && apt-get install curl -y && curl https://sh.rustup.rs -sSf | sh -s -- -y
EOF


docker run --rm -v "${PWD}":/workspace -w /workspace img-build-ubuntu:"${user}" /bin/sh -c "
    . $HOME/.cargo/env &&
    rustup default stable &&
    cargo build --release &&
    chmod 777 target/*
"



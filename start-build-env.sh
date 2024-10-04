script_dir=$(dirname "$(realpath "$0")")

user=$(whoami)

docker build -t img-build-ubuntu:"${user}" -f- . <<EOF
FROM ubuntu:20.04
RUN apt-get update -y && apt-get install curl -y && apt-get install build-essential && curl https://sh.rustup.rs -sSf | sh -s -- -y
EOF

echo ${script_dir}

docker run --rm -v "${script_dir}":/workspace -w /workspace img-build-ubuntu:"${user}" /bin/sh -c "
    echo $PWD
    ls .
    . /root/.cargo/env &&
    rustup default stable &&
    cargo build --release &&
    chmod 777 target/*
"



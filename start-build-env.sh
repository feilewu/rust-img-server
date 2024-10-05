script_dir=$(dirname "$(realpath "$0")")

echo "当前所在目录： "${script_dir}

ls -l ./

user=$(whoami)

echo $user

docker build -t img-build-ubuntu:"${user}" -f- . <<EOF
FROM ubuntu:20.04
RUN apt-get update -y && apt-get install curl -y && apt-get install build-essential -y && curl https://sh.rustup.rs -sSf | sh -s -- -y
EOF

echo ${script_dir}

docker run --rm -u "${user}" -v "${script_dir}":/workspace -w /workspace img-build-ubuntu:"${user}" /bin/sh -c "
    echo $PWD
    ls -l ./
    . ${HOME}/.cargo/env &&
    rustup default stable &&
    cargo build --release &&
    chmod 777 target/*
"



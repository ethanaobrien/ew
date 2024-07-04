FROM docker.io/library/debian:latest

RUN apt update && apt install -y curl npm libssl-dev perl git
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash
RUN apt update && apt install -y nodejs
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain stable --profile minimal
RUN git clone https://github.com/ethanaobrien/ew.git

WORKDIR /ew/webui/

RUN npm i && npm run build

WORKDIR /ew/

RUN . "$HOME/.cargo/env" && cargo build --release
RUN mkdir /root/ew/ && cp target/release/ew /root/ew/ew

COPY ./start.sh /root/ew/start.sh

RUN chmod +x /root/ew/start.sh

WORKDIR /

RUN rm -rf /ew/

ENTRYPOINT ["/root/ew/start.sh"]

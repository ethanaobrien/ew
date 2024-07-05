FROM docker.io/library/debian:latest as builder

# First - build

RUN apt update && apt install -y curl libssl-dev perl git
RUN curl -fsSL https://deb.nodesource.com/setup_22.x | bash
RUN apt update && apt install -y nodejs
RUN curl -sSf https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain stable --profile minimal

WORKDIR /ew/

COPY ./ ./

WORKDIR /ew/webui/

RUN npm i && npm run build

WORKDIR /ew/

RUN . "$HOME/.cargo/env" && cargo build --release

## Second - sort stuff idk

FROM docker.io/library/debian:bookworm-slim

RUN mkdir -p /root/ew/
COPY --from=builder /ew/target/release/ew /root/ew/ew
COPY ./docker/start.sh /root/ew/start.sh

RUN chmod +x /root/ew/start.sh

ENTRYPOINT ["/root/ew/start.sh"]

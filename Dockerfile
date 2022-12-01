# https://www.artificialworlds.net/blog/2020/04/22/creating-a-tiny-docker-image-of-a-rust-project/

FROM rust as builder
WORKDIR /usr/src

RUN apt-get update && \
    apt-get dist-upgrade -y && \
    apt-get install -y musl-tools && \
    rustup target add x86_64-unknown-linux-musl

RUN USER=root cargo new iv_rhash_bot
WORKDIR /usr/src/iv_rhash_bot
COPY Cargo.toml Cargo.lock ./
RUN cargo fetch

COPY src ./src
RUN cargo build --target x86_64-unknown-linux-musl --release

FROM scratch
COPY --from=builder /usr/src/iv_rhash_bot/target/x86_64-unknown-linux-musl/release/iv_rhash_bot .
CMD ["./iv_rhash_bot"]

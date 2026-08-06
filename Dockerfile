FROM lukemathwalker/cargo-chef:latest-rust-alpine AS chef
WORKDIR /app

FROM chef AS planner
COPY . .
RUN cargo chef prepare --recipe-path recipe.json

FROM chef AS builder
COPY --from=planner /app/recipe.json recipe.json
RUN cargo chef cook --release --package planpool-server --recipe-path recipe.json
COPY . .
RUN cargo build --release --package planpool-server

FROM alpine:3.22 AS runtime
RUN apk add --no-cache su-exec \
    && addgroup -S planpool && adduser -S planpool -G planpool \
    && mkdir /data && chown planpool:planpool /data
COPY --from=builder /app/target/release/planpool /usr/local/bin/planpool
COPY --chmod=755 docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
ENV PLANPOOL_DATA_DIR=/data
EXPOSE 8080
ENTRYPOINT ["docker-entrypoint.sh"]

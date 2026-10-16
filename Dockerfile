FROM rust:1.91-bookworm AS build
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY crates ./crates
COPY src-tauri ./src-tauri
COPY tests ./tests
RUN cargo build --locked --release -p harness-scope-service
FROM debian:bookworm-slim
RUN apt-get update && apt-get install -y --no-install-recommends ca-certificates && rm -rf /var/lib/apt/lists/* && useradd --system --uid 10001 --create-home scope && mkdir /data && chown scope /data
COPY --from=build /app/target/release/harness-scope-service /usr/local/bin/harness-scope-service
USER scope
VOLUME ["/data"]
EXPOSE 8787
ENTRYPOINT ["harness-scope-service"]
CMD ["--db","/data/scope.db","--bind","0.0.0.0:8787"]

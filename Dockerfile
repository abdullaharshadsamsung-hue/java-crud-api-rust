FROM rust:1.90-slim

WORKDIR /app

COPY . .

RUN cargo build --release

EXPOSE 8080

CMD ["sh", "-c", "/app/target/release/migrated-service"]

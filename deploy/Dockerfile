# syntax=docker/dockerfile:1
# Imagen de Mythos en tres etapas: build de la UI (Node), build del backend (Rust, con las
# dependencias cacheadas en su propia capa) y runtime liviano con ffmpeg (Debian trae h264_nvenc;
# las librerías de NVIDIA las inyecta el runtime de nvidia al arrancar).
# Contexto = raíz del repo:  docker build -f deploy/Dockerfile .

# ---------- 1. UI ----------
FROM node:22-alpine AS web
WORKDIR /web
COPY apps/web/package.json apps/web/package-lock.json ./
RUN npm ci --no-audit --no-fund
COPY apps/web/ ./
RUN npm run build

# ---------- 2. backend ----------
FROM rust:1-trixie AS backend
WORKDIR /src
# Primero solo los manifiestos con fuentes vacías: esta capa (todas las dependencias compiladas)
# se reusa mientras no cambien Cargo.toml/Cargo.lock.
COPY backend/Cargo.toml backend/Cargo.lock ./
COPY backend/server/Cargo.toml server/Cargo.toml
COPY backend/media/Cargo.toml media/Cargo.toml
RUN mkdir -p server/src media/src \
 && echo 'fn main() {}' > server/src/main.rs \
 && touch media/src/lib.rs \
 && cargo build --release --locked -p mythos \
 && rm -rf server/src media/src
COPY backend/server/src server/src
COPY backend/server/assets server/assets
COPY backend/media/src media/src
# `touch` + borrar las huellas de los crates propios: COPY conserva fechas viejas y cargo podría
# creer que el binario vacío de arriba sigue vigente.
RUN touch server/src/main.rs server/src/lib.rs media/src/lib.rs \
 && rm -rf target/release/.fingerprint/mythos-* \
 && cargo build --release --locked -p mythos \
 && cp target/release/mythos /mythos

# ---------- 3. runtime ----------
FROM debian:trixie-slim
RUN apt-get update \
 && apt-get install -y --no-install-recommends ffmpeg ca-certificates tini \
 && rm -rf /var/lib/apt/lists/* \
 && groupadd --system --gid 1000 mythos \
 && useradd --system --uid 1000 --gid 1000 --home-dir /data --no-create-home mythos \
 && mkdir -p /data /app \
 && chown mythos:mythos /data
COPY --from=backend /mythos /usr/local/bin/mythos
COPY --from=web /web/dist /app/web
ENV MYTHOS_BIND=0.0.0.0:7700 \
    MYTHOS_DATA=/data \
    MYTHOS_WEB_DIR=/app/web \
    MYTHOS_PEER_PORT=42069 \
    RUST_LOG=info
USER mythos
WORKDIR /data
VOLUME /data
EXPOSE 7700 42069/tcp 42069/udp
HEALTHCHECK --interval=30s --timeout=5s --start-period=20s --retries=3 CMD ["mythos", "health"]
ENTRYPOINT ["/usr/bin/tini", "--", "mythos"]

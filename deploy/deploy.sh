#!/usr/bin/env bash
# Despliegue de Mythos en el server Linux (idempotente: se puede correr las veces que haga falta).
#   1. git pull
#   2. crea deploy/.env si no existe (con una contraseña de admin aleatoria que se muestra una vez)
#   3. docker compose up -d --build
#   4. espera a que el healthcheck dé "healthy"
#   5. publica la app en la tailnet con `tailscale serve` si todavía no está publicada
# Uso:  ./deploy/deploy.sh            (desde cualquier carpeta)
#       MYTHOS_ADMIN_USER=martin ./deploy/deploy.sh   (nombre del primer admin; por defecto "admin")
set -euo pipefail

DEPLOY_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_DIR="$(dirname "$DEPLOY_DIR")"
PORT=7700
CONTAINER=mythos
HEALTH_TIMEOUT_S=180

say() { printf '\033[1;36m==>\033[0m %s\n' "$*"; }
warn() { printf '\033[1;33m!!\033[0m %s\n' "$*" >&2; }
die() { printf '\033[1;31mxx\033[0m %s\n' "$*" >&2; exit 1; }

# sudo solo si hace falta (si ya somos root, o el comando anda sin él, no se usa).
as_root() { if [ "$(id -u)" -eq 0 ]; then "$@"; else sudo "$@"; fi; }

command -v docker >/dev/null || die "falta docker"
docker compose version >/dev/null 2>&1 || die "falta el plugin 'docker compose'"

# --- 1. código -----------------------------------------------------------------------------
say "Actualizando el repo"
git -C "$REPO_DIR" pull --ff-only

# --- 2. configuración ----------------------------------------------------------------------
cd "$DEPLOY_DIR"
mkdir -p data
# El contenedor corre con uid/gid 1000: la carpeta de datos tiene que ser suya.
if [ "$(stat -c '%u:%g' data)" != "1000:1000" ]; then
  say "Dando la carpeta data/ al uid 1000 (el usuario del contenedor)"
  as_root chown -R 1000:1000 data
fi

if [ ! -f .env ]; then
  say "Creando deploy/.env"
  admin_user="${MYTHOS_ADMIN_USER:-admin}"
  admin_pass="$(head -c 18 /dev/urandom | base64 | tr -d '/+=
')"
  sed -e "s/^MYTHOS_ADMIN_USER=.*/MYTHOS_ADMIN_USER=${admin_user}/" \
      -e "s/^MYTHOS_ADMIN_PASSWORD=.*/MYTHOS_ADMIN_PASSWORD=${admin_pass}/" \
      .env.example > .env
  # Con GPU NVIDIA, nvenc; si no, CPU.
  if ! command -v nvidia-smi >/dev/null || ! nvidia-smi >/dev/null 2>&1; then
    warn "no se encontró una GPU NVIDIA: MYTHOS_HWACCEL=none (ver 'Sin GPU' en deploy/README.md)"
    sed -i 's/^MYTHOS_HWACCEL=.*/MYTHOS_HWACCEL=none/' .env
  fi
  chmod 600 .env
  if [ ! -f data/mythos.db ]; then
    printf '\n\033[1;32m  Primer admin:  %s  /  %s\033[0m\n' "$admin_user" "$admin_pass"
    printf '  (se muestra una sola vez; está en deploy/.env hasta que lo borres de ahí)\n\n'
  else
    warn "ya hay una base de datos: el admin de .env no se usa (los usuarios existentes siguen)"
  fi
fi

# --- 3. contenedor -------------------------------------------------------------------------
say "Construyendo y levantando el contenedor"
docker compose up -d --build

# --- 4. healthcheck ------------------------------------------------------------------------
say "Esperando el healthcheck"
deadline=$((SECONDS + HEALTH_TIMEOUT_S))
while :; do
  status="$(docker inspect --format '{{if .State.Health}}{{.State.Health.Status}}{{else}}{{.State.Status}}{{end}}' "$CONTAINER" 2>/dev/null || echo missing)"
  case "$status" in
    healthy) break ;;
    unhealthy|exited|dead|missing)
      docker compose logs --tail 50 "$CONTAINER" >&2 || true
      die "el contenedor quedó '$status'" ;;
  esac
  [ "$SECONDS" -lt "$deadline" ] || { docker compose logs --tail 50 "$CONTAINER" >&2 || true; die "timeout esperando el healthcheck ($status)"; }
  sleep 2
done
say "Mythos responde en http://127.0.0.1:${PORT}"

# --- 5. tailscale serve --------------------------------------------------------------------
if command -v tailscale >/dev/null; then
  ts() { tailscale "$@" 2>/dev/null || as_root tailscale "$@"; }
  if ts serve status 2>/dev/null | grep -q ":${PORT}"; then
    say "tailscale serve ya publica el puerto ${PORT}"
  else
    say "Publicando en la tailnet (tailscale serve --https=${PORT})"
    ts serve --bg --https="${PORT}" "http://127.0.0.1:${PORT}"
  fi
  dns="$(ts status --json 2>/dev/null | sed -n 's/.*"DNSName": *"\([^"]*\)\.".*/\1/p' | head -n1)"
  if [ -n "$dns" ]; then say "Listo: https://${dns}:${PORT}"; fi
else
  warn "tailscale no está instalado: la app queda solo en http://127.0.0.1:${PORT}"
fi

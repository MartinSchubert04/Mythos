# CLAUDE.md

Mythos: cliente tipo Stremio para uso personal. El servidor (en `MRTN-SERVER`, Docker) descarga los
torrents que devuelve Torrentio con su propio motor (librqbit) y le manda el video a los clientes por
HTTP; si el formato no lo soporta el navegador, lo convierte en vivo con ffmpeg + NVENC. Usuarios con
login (nadie entra sin cuenta), modo admin para crearlos. Interfaz web y desktop (Tauri 2) iguales.

## Dónde está cada decisión

| Pregunta | Archivo |
|---|---|
| Contrato HTTP entre UI y servidor, API del crate de media | `docs/API.md` |
| Cómo tiene que verse la interfaz | `DESIGN.md` (portado de Ethereon) |
| Despliegue en el server, puertos | `deploy/README.md` |

## Estructura

```
backend/                  workspace de Rust (lo único que entra al contenedor)
  server/                 binario `mythos`: axum, SQLite (sqlx), auth, admin, catálogo, progreso,
                          sirve la UI compilada y monta el router de media
  media/                  crate `mythos-media`: motor torrent (librqbit), streaming con Range,
                          ffmpeg (HLS, NVENC), subtítulos internos. No sabe nada de usuarios.
apps/web/                 la interfaz (Svelte 5 + Vite + TS). La misma para web y desktop.
apps/desktop/             Tauri 2: solo la ventana; carga el build de apps/web
deploy/                   Dockerfile, docker-compose.yml, README de despliegue
```

## Puertos (MRTN-SERVER)

- `127.0.0.1:7700` la app (API + UI). Afuera solo por `tailscale serve --https=7700`
  → `https://mrtn-server.tailbce521.ts.net:7700` (solo tailnet).
- `42069` TCP/UDP: peers de BitTorrent (abrirlo en el router para más velocidad).
- No usar: 22, 53, 443, 1883, 3000, 4000, 4517, 5678, 6052, 6080, 8080, 8090, 8123, 8443, 8765,
  8787, 8788, 10200, 10300, 11987, 51515, 61208 (ya están ocupados).

## Comandos

```powershell
# backend
cd backend; cargo run -p mythos      # http://127.0.0.1:7700 (usa ./data)
cd backend; cargo test
# interfaz
cd apps/web; npm run dev             # http://localhost:5173, proxy de /api a :7700
cd apps/web; npm run check
# desktop
cd apps/desktop; npm run tauri dev
```

## Commits y versiones

Repo: https://github.com/MartinSchubert04/Mythos (público: nada de secretos, `.env` y `data/` fuera).

- Conventional commits, en español: `feat: …`, `fix: …`, `docs: …`, `refactor: …`, `chore: …`
  (con scope si ayuda: `feat(player): …`). En el cuerpo, una línea por cambio.
- Cada `feat` o `fix` terminado (tests y check en verde) se commitea y pushea sin esperar a que lo
  pidan, subiendo la versión con semver (`feat` → menor, `fix` → parche; mayor solo preguntando)
  y con tag `vX.Y.Z` sobre ese commit. `docs`/`chore` sin tag.
- La versión está en `backend/server/Cargo.toml`, `backend/media/Cargo.toml`,
  `apps/web/package.json`, `apps/desktop/package.json`, `apps/desktop/src-tauri/Cargo.toml` y
  `apps/desktop/src-tauri/tauri.conf.json` (todas iguales).

## Reglas

- **Temas solo con variables** (igual que Ethereon): colores en `apps/web/src/styles/themes.css`,
  nunca fijos en componentes.
- **Animaciones solo por `apps/web/src/lib/motion.ts`** (respetan "reducir animaciones").
- **Ninguna ruta de `/api` sin login**, salvo `POST /api/auth/login` y `GET /api/health`.
  Las rutas de media aceptan el token también en `?t=` porque `<video>` no manda headers.
- Las claves de Real-Debrid y las contraseñas nunca vuelven en ninguna respuesta.
- Docs y comentarios en español; textos de la interfaz en inglés.

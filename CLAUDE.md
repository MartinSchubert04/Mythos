# CLAUDE.md

Mythos: cliente tipo Stremio para uso personal. El servidor (en `MRTN-SERVER`, Docker) descarga los
torrents que devuelve Torrentio con su propio motor (librqbit) y le manda el video a los clientes por
HTTP; si el formato no lo soporta el navegador, lo convierte en vivo con ffmpeg + NVENC. Usuarios con
login (nadie entra sin cuenta), modo admin para crearlos. Interfaz web, desktop y Android (Tauri 2)
iguales.

## Dónde está cada decisión

| Pregunta | Archivo |
|---|---|
| Contrato HTTP entre UI y servidor, API del crate de media | `docs/API.md` |
| Cómo tiene que verse la interfaz | `DESIGN.md` (portado de Ethereon) |
| Despliegue en el server, puertos | `deploy/README.md` |
| App de Android: shell, interfaz del server opcional, actualizaciones, publicar | `docs/mobile.md` |
| Modo local (server embebido en desktop y Android, sincronización, Auto/Solo server/Solo local) | `docs/local-mode.md` |
| Android TV / TV box: 32 y 64 bits, control remoto, escala de 10 pies, perfiles, WebView mínimo | `docs/mobile.md` ("TV") |
| Samsung (Tizen): la app de la tele, certificados de Samsung, compilar, instalar por WiFi | `docs/tizen.md` |
| Desktop: actualizaciones desde los Releases (cuándo se instala, relanzar) | `apps/desktop/src-tauri/src/updater.rs` |
| Desktop: vidrio esmerilado (Acrylic + opacidad sobre cualquier tema) | `apps/web/src/lib/glass.svelte.ts` |
| "Top rated": de dónde sale (datasets de IMDb), cada cuánto se baja, dónde se guarda | `backend/server/src/imdb.rs`, `docs/API.md` ("Top rated") |
| Rendimiento: tamaño de las imágenes (`/api/media/img`), modo liviano en equipos flojos, qué evitar | `apps/web/src/lib/images.ts`, `apps/web/src/lib/perf.ts`, `DESIGN.md` (5b y 7) |

## Estructura

```
backend/                  workspace de Rust (lo único que entra al contenedor)
  server/                 binario `mythos`: axum, SQLite (rusqlite), auth, admin, catálogo, progreso,
                          sirve la UI compilada y monta el router de media
  media/                  crate `mythos-media`: motor torrent (librqbit), streaming con Range,
                          ffmpeg (HLS, NVENC), subtítulos internos. No sabe nada de usuarios.
apps/web/                 la interfaz (Svelte 5 + Vite + TS). La misma para web, desktop y Android.
apps/desktop/             Tauri 2: solo la ventana; carga el build de apps/web
apps/mobile/              Tauri 2 para Android: carga el build de apps/web; plugin/ (Rust + Kotlin)
                          con pantalla completa, atras, actualizaciones del APK
apps/local-server/        plugin de Tauri `mythos-local` (desktop y Android): el server embebido del
                          modo local (docs/local-mode.md)
apps/tizen/               app web de Tizen (.wgt) para las teles Samsung: elige el server y abre su
                          interfaz (docs/tizen.md)
deploy/                   Dockerfile, docker-compose.yml, README de despliegue
```

## Puertos (MRTN-SERVER)

- `127.0.0.1:7700` la app (API + UI). Afuera solo por `tailscale serve --https=7700`
  → `https://<tu-server>.ts.net:7700` (solo tailnet). Desktop y Android piden esa URL en el login
  la primera vez y la guardan: ningún build lleva la dirección (los Releases son públicos).
  `VITE_DEFAULT_SERVER` (`apps/web/.env.local`, fuera del repo) es solo para probar en la PC.
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
# android (ANDROID_HOME, NDK_HOME y JAVA_HOME: ver docs/mobile.md)
cd apps/mobile; npm run build:emulator
```

## Versiones: commit, tag y push

Repo: https://github.com/MartinSchubert04/Mythos (público: nada de secretos, `.env` y `data/` fuera).

Cada fix o feature terminado (tests y check en verde) se publica sin esperar a que lo pidan:
subir la versión, commitear, tagear y pushear `main` y el tag. El tag dispara los workflows que
arman el Release (instalador de Windows + `latest.json`, APK + `latest-android.json`), y de ahí
se actualizan solas las apps instaladas (desktop: `apps/desktop/src-tauri/src/updater.rs`;
Android: `docs/mobile.md`).

- **Qué número sube** (semver, `MAYOR.MENOR.PARCHE`):
  - `x.y.Z` (parche): arreglos y retoques chicos de algo que ya existía (un texto, un ancho,
    un bug, un valor por defecto).
  - `x.Y.0` (menor): algo nuevo que se puede usar (una opción, una pantalla, una función), sin
    romper nada.
  - `X.0.0` (mayor): rompe algo de lo que ya existe (la API entre apps y server sin
    compatibilidad, la base sin migración, quitar una función). Antes de subirla, preguntar.
- **Dónde está la versión** (todas iguales): `backend/server/Cargo.toml`,
  `backend/media/Cargo.toml`, `apps/web/package.json`, `apps/desktop/package.json`,
  `apps/desktop/src-tauri/Cargo.toml`, `apps/desktop/src-tauri/tauri.conf.json`,
  `apps/mobile/package.json`, `apps/mobile/src-tauri/Cargo.toml`,
  `apps/mobile/src-tauri/tauri.conf.json`, `apps/mobile/plugin/Cargo.toml`,
  `apps/local-server/Cargo.toml`, `apps/tizen/config.xml` (`version` del `<widget>`), y las entradas de esos paquetes en los `Cargo.lock` y
  `package-lock.json` (ojo con reemplazar a ciegas: otras dependencias pueden tener el mismo
  número).
- **Commit**: `vX.Y.Z: <resumen>` y en el cuerpo una línea por cambio, en español, terminando
  con la versión (ver `git log`). Tag `vX.Y.Z` sobre ese commit.
- Cambios que no son ni fix ni feature (docs, este archivo, CI) se commitean y pushean sin tag.
- Las claves de firma (APK y actualizaciones de desktop) viven fuera del repo, en
  `~/.mythos-signing/`, y como secrets del repo. Si se pierden, las apps instaladas no se pueden
  actualizar sin desinstalar.

## Reglas

- **Temas solo con variables** (igual que Ethereon): colores en `apps/web/src/styles/themes.css`,
  nunca fijos en componentes.
- **Animaciones solo por `apps/web/src/lib/motion.ts`** (respetan "reducir animaciones").
- **Ninguna ruta de `/api` sin login**, salvo `POST /api/auth/login` y `GET /api/health`.
  Las rutas de media aceptan el token también en `?t=` porque `<video>` no manda headers.
- Las claves de Real-Debrid y las contraseñas nunca vuelven en ninguna respuesta.
- Docs y comentarios en español; textos de la interfaz en inglés.

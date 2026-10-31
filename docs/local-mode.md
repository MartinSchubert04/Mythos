# Modo local (server embebido en las apps)

Las apps de escritorio (Tauri 2, Windows) y de celular (Tauri 2, Android) pueden funcionar sin el
server de casa. Llevan adentro el mismo server de Mythos (el crate `mythos`, `backend/server`)
corriendo en el propio dispositivo, en modo `local`: catálogo de Cinemeta, streams de Torrentio,
torrents bajados y reproducidos en el dispositivo, progreso, biblioteca y settings. Misma cuenta
que en el server. Cuando el server vuelve, se sincroniza todo solo.

Contrato HTTP en `docs/API.md` (secciones "Sincronización", "Modo local" y "Embeber el server").

## Arquitectura

```
┌──────────────────────── app (Tauri) ─────────────────────────┐
│  webview (apps/web)                                          │
│    │  API base = server remoto  (online)                     │
│    │           = http://127.0.0.1:<puerto>  (local)          │
│    ▼                                                         │
│  shell Rust ── mythos::run(Config::local(dir)) ──► Handle    │
│                 │  axum en 127.0.0.1:0, SQLite propia,       │
│                 │  librqbit, ffmpeg si hay                   │
│                 └── sync (push/pull) ──────────┐             │
└────────────────────────────────────────────────│─────────────┘
                                                 ▼ (Tailscale)
                              server de casa (modo server, Docker)
                              GET/POST /api/sync, /api/auth/login
```

- **El shell** arranca el server local al abrir la app (`mythos::run`) y le pasa a la UI la URL
  (`Handle::base_url()`, puerto elegido por el sistema). Al cerrar, `Handle::shutdown()`. En
  **Auto** arranca ~8 s después (la UI tampoco lo pide antes): en una TV box o un celular flojo
  competiría por CPU y red con el primer render. Si hace falta antes (login por el local, server
  de casa caído), el comando `info` lo arranca en el momento. En Solo local arranca enseguida.
- **La UI** habla con el server remoto mientras se lo alcance (streaming con NVENC, caché grande)
  y con el local cuando no. No implementa sincronización: solo elige a quién hablarle.
- **El server local** guarda los datos en su propia SQLite y sincroniza con el remoto por su
  cuenta: cada 60 s (`sync_interval`) si el remoto responde, ~5 s después de cada cambio local,
  y a pedido (`POST /api/local/sync`).

## Modos que elige el usuario

| Modo | Shell | UI |
|---|---|---|
| **Auto** (recomendado) | arranca el server local con `remote_url` = server de casa | login por el local (`POST {local}/api/auth/login` con `remote_url`); usa el remoto si `offline: false`, el local si no; cambia sola (abajo) |
| **Solo server** | no hace falta arrancar el local | como hasta ahora: todo contra el remoto. Si no responde, error |
| **Solo local** | arranca el local (con `remote_url` para sincronizar, o `None` para no tocar la red del server) | todo contra el local, siempre (streaming en el dispositivo). El login igual valida online si puede (`offline: false`) |

Cambiar de modo no pierde datos: el local y el remoto convergen por la sincronización. Pasar de
"Solo server" a "Auto"/"Solo local" pide la contraseña una vez (hace falta un login online a
través del local para enrolar la cuenta en el dispositivo).

## Flujos

### Primer login (online)

1. La UI manda `POST {local}/api/auth/login {username, password, device, remote_url}`.
2. El local hace el login contra el remoto. Si acepta: crea/actualiza la cuenta local (mismo
   nombre, verificación propia de la contraseña), guarda una sesión del remoto para sincronizar
   (`local_link`), abre una sesión local y arranca la primera sincronización (baja progreso,
   biblioteca y settings).
3. Responde `{token, user, offline: false, remote: {url, token, user}}`. La UI guarda los dos
   tokens y usa el remoto.

Si el remoto rechaza la contraseña → 401 con su mensaje (no se prueba la local). Si la cuenta está
deshabilitada en el server, también se deshabilita en el dispositivo.

### Caída del server ("Server unavailable, continuing locally")

- Al iniciar sesión: si el remoto no se alcanza (red, timeout de conexión de 4 s, 5xx), el local
  valida contra su verificación y responde `offline: true`. La UI muestra el aviso y usa el local.
- Durante el uso: si un request al remoto falla **por red** (no por un status HTTP) o
  `GET {remote}/api/health` falla dos veces seguidas, la UI muestra el aviso y pasa al local con
  el token local. Recomendado: no cambiar en medio de una película; cambiar en la próxima
  navegación (la película sigue mientras el remoto la sirva).
- El local tiene los datos hasta su última sincronización (≤ 60 s de atraso). Lo que el usuario
  haga después (progreso, biblioteca, settings) queda en el local y se sube al volver.

### Vuelta del server (reconexión)

1. Mientras está en local, la UI consulta `GET {local}/api/local/status` cada 15–30 s.
2. Cuando `remote_reachable` es `true`: `POST {local}/api/local/sync` (sube lo hecho offline y
   baja lo nuevo). Con `200`, la UI vuelve al remoto con el token remoto que ya tenía.
3. Si `remote_signed_in` es `false` (la sesión del remoto venció o se revocó) o el sync da 401:
   hace falta un login online (pedir la contraseña) para seguir sincronizando. Mientras tanto se
   puede seguir en local.

El server local además sincroniza solo en segundo plano: aunque la UI no haga nada, los cambios
llegan al remoto en cuanto se lo alcanza.

### Conflictos

Last-write-wins por registro con `updated_ms` (reloj del dispositivo), lápidas para borrados,
idempotente, incremental por cursor (ver `docs/API.md` → "Sincronización"). Si el mismo episodio
se miró en dos dispositivos sin conexión, queda la posición del último que escribió. Un reloj muy
adelantado en un dispositivo hace que sus cambios ganen hasta que pase esa hora; cada escritura
nueva usa `max(ahora, anterior + 1)` para que lo último que hizo el usuario en un dispositivo no
pierda contra lo que ese dispositivo ya tenía.

## Seguridad

### Credenciales offline: verificación propia, no el hash del server

Para validar la contraseña sin red hacía falta algo en el dispositivo. Opciones:

1. Bajar del server el hash argon2 del usuario ("bundle de credenciales").
2. **Calcular una verificación propia en el dispositivo** (elegida).

El server local hace el login contra el remoto con la contraseña que tipeó el usuario; solo si el
remoto la acepta calcula un argon2id nuevo, con sal propia, y lo guarda en su SQLite. Por qué:

- **El hash del server nunca sale del server**: no hay ninguna ruta que lo exporte (menos
  superficie de ataque, la regla "las contraseñas nunca vuelven en ninguna respuesta" se cumple
  sin excepciones) y un cambio de parámetros de argon2 en el server no rompe a los clientes.
- Robar el dispositivo da lo mismo que daría el bundle (una verificación para fuerza bruta
  offline) pero no permite nada contra el server: el server nunca acepta hashes, solo
  contraseñas.
- No hace falta ninguna ruta nueva en el server para esto: alcanza el login normal.

Consecuencias: el primer login en cada dispositivo tiene que ser online. Un cambio de contraseña
en el server llega al dispositivo en el próximo login online (hasta entonces offline vale la
vieja); por eso `PUT /api/me/password` da 403 en modo local.

### La API local en 127.0.0.1

En Android cualquier app del teléfono puede conectarse a `127.0.0.1:<puerto>`. Por eso:

- Todas las rutas piden login igual que en el server (salvo `POST /api/auth/login` y
  `GET /api/health`), con el mismo rate limit (10 fallos / 5 min).
- Solo se aceptan requests con `Host` de loopback: una página web que haga apuntar su dominio a
  127.0.0.1 (DNS rebinding) recibe 403. CORS solo para los orígenes de Tauri y Vite.
- Sin admin (403 en `/api/admin/**`), sin métricas.

### Lo que se guarda en el dispositivo

En `data_dir` (carpeta privada de la app): `mythos.db` con la verificación argon2 de la
contraseña, las sesiones locales (solo el sha256 de cada token) y la sesión del remoto que usa la
sincronización (`local_link.remote_token`, en claro: es una sesión revocable desde
`/api/me/sessions` del server, aparece como el dispositivo que se logueó). El logout local, si es
la última sesión de esa cuenta en el dispositivo, cierra también esa sesión remota.

### Real-Debrid

Regla del proyecto: la clave de RD nunca vuelve en ninguna respuesta. Por eso **no se
sincroniza** (`SettingsRec` no la tiene, y aplicar settings remotos no toca la local). El modo
local anda sin RD (torrents comunes en el dispositivo); si el usuario quiere RD también offline,
la carga en la app (`PUT /api/me/settings {rd_key}` contra el local) y queda solo en ese
dispositivo.

## Reproducción en el dispositivo y ffmpeg

- Los torrents se bajan en el dispositivo (`data_dir/torrents`). Sin tope de tamaño: cada película
  se borra a las 24 h de su último uso (`cache_max_age`; nunca la que se está viendo, y lo que
  venció con la app cerrada se borra al abrirla). Con menos de 2 GB libres se borran antes, y con
  menos de 1 GB se pausa la descarga. El primer play tarda lo mismo que en el server; no hay NVENC.
- ffmpeg es opcional. El shell puede pasar la ruta de un ffmpeg (en Windows, si está en el PATH,
  `Config::local` ya lo usa). Sin ffmpeg: solo reproducción directa, sondeo con el lector propio
  de cabeceras MP4/MKV, `PlayInfo.transcode_available: false` y `direct_ok` dice si el
  reproductor lo va a poder abrir. `/api/streams` con `?caps=` hunde lo que no es directo. La UI
  tendría que mandar `caps` reales del dispositivo (Android: `<video>` de WebView suele hacer
  h264/hevc/vp9/av1 + aac/opus/flac en mp4/webm; mkv depende del equipo) y, si `direct_ok` es
  `false`, ofrecer el siguiente stream.
- Subtítulos externos (OpenSubtitles) andan igual; los internos necesitan ffmpeg.

## Android

- **Carpeta de datos**: la privada de la app (`app.path().app_data_dir()` en Tauri =
  `/data/user/0/<paquete>/files`, o `cache_dir` para los torrents si se quiere que el sistema
  pueda liberarlos). No usar almacenamiento compartido.
- **Puertos**: el server en `127.0.0.1:0` (el shell le pasa a la UI el puerto real); peers de
  BitTorrent en puerto efímero (`peer_port = 0`); en redes móviles UPnP no hace nada y no
  importa. La UI le habla a `http://127.0.0.1:<puerto>` desde `http://tauri.localhost`: hace
  falta permitir HTTP en claro **solo** para loopback con un
  `network_security_config.xml` (`<domain-config cleartextTrafficPermitted="true">` con
  `127.0.0.1` y `localhost`), además del permiso `INTERNET`.
- **TLS**: los clientes HTTP propios usan las raíces de Mozilla embebidas
  (`mythos_media::http_client_builder`), porque el verificador de plataforma de rustls necesita
  inicializarse por JNI. librqbit tiene su propio cliente HTTP (trackers `https://`); sin esa
  inicialización esos trackers fallan, pero los UDP y el DHT andan, que es lo que más usa
  Torrentio. Si se quiere completo: el shell llama a
  `rustls_platform_verifier::android::init_hosted` al arrancar y agrega el componente de Gradle
  `rustls-platform-verifier-android`.
- **Batería y segundo plano**: Android congela o mata procesos en segundo plano. Mientras se
  reproduce o se baja algo, el shell debería tener un *foreground service*
  (`mediaPlayback`/`dataSync`) con notificación; sin eso, apagar la pantalla puede cortar la
  descarga. Sin reproducción, no hace falta: el motor pausa los torrents ociosos solo (15 min) y
  la sincronización es un request chico cada 60 s solo si el server responde. Al pasar a segundo
  plano mucho tiempo, el shell puede llamar a `shutdown()` y volver a arrancar al volver (tarda
  ~0,5 s; los datos quedan).
- **Tailscale**: el remoto se alcanza solo con la VPN de Tailscale prendida; si no, el modo Auto
  cae a local (es el caso normal de "server no disponible").
- **Logs**: la biblioteca usa `tracing` y no inicializa nada; en Android el shell puede mandar
  los eventos a logcat (p. ej. `tracing-android` o `android_logger`).

## Compilar para Windows y Android

Windows (MSVC) es el target nativo: `cargo build -p mythos --lib`.

Android (`aarch64-linux-android`), desde Windows con el NDK de Android Studio
(`%LOCALAPPDATA%\Android\Sdk\ndk\<28.x o 30.x>`):

```bash
NDK="$LOCALAPPDATA/Android/Sdk/ndk/28.0.13004108"
TC="$NDK/toolchains/llvm/prebuilt/windows-x86_64/bin"
export ANDROID_NDK_HOME="$NDK"
export CC_aarch64_linux_android="$TC/aarch64-linux-android24-clang.cmd"
export CXX_aarch64_linux_android="$TC/aarch64-linux-android24-clang++.cmd"
export AR_aarch64_linux_android="$TC/llvm-ar.exe"
export CARGO_TARGET_AARCH64_LINUX_ANDROID_LINKER="$TC/aarch64-linux-android24-clang.cmd"
rustup target add aarch64-linux-android
cd backend && cargo build --target aarch64-linux-android -p mythos --lib
```

No hizo falta nada más: SQLite (`rusqlite` con `bundled`), `ring` y `aws-lc-sys` (los proveedores
de rustls que traen librqbit y reqwest) compilan con el clang del NDK sin CMake ni OpenSSL. Lo
único propio de Android en el código es la verificación TLS (arriba). Con Tauri, `tauri android
build` pone estas variables solo.

## Cómo está hecho en las apps

**Shell** (`apps/local-server`, plugin de Tauri `mythos-local`, el mismo en desktop y Android):

- Guarda el modo en `<app_local_data_dir>/local-mode` (`auto` por defecto). En `auto` y `local`
  arranca `mythos::run(Config::local(<app_local_data_dir>/local))` en segundo plano apenas abre la
  app (la ventana no espera; ~1–2 s en debug, el server local queda listo para cuando haga falta).
  En `server` no arranca nada.
- Comandos: `info` → `{mode, base_url, transcode_available, error}` (espera si está arrancando) y
  `set_mode(mode)` (lo guarda; `server` apaga el local, `auto`/`local` lo arrancan).
- ffmpeg: no viene con la app. En Windows, si `ffmpeg` y `ffprobe` están en el PATH se usan; si
  no, rutas vacías (solo directo, el motor ni intenta lanzarlos). En Android, nunca. Los procesos
  de ffmpeg se lanzan sin ventana de consola (`CREATE_NO_WINDOW`, `mythos_media::util::tool_command`).
- Al salir (`RunEvent::Exit`): `Handle::shutdown()`.
- `tracing`: a stderr en desktop, a logcat (etiqueta `mythos`) en Android (`init_tracing`).
- Android: `rustls_platform_verifier::android::init_with_env` con el contexto de la app (de
  `ndk-context`, que deja tao) y la parte de Kotlin `org.rustls:rustls-platform-verifier` en
  Gradle (misma versión que `rustls-platform-verifier-android` en el `Cargo.lock`, regla de
  ProGuard en `proguard-rules.pro`). Sin eso el primer tracker `https://` de librqbit entraba en
  pánico (y en release, con `panic = "abort"`, cerraba la app).

**UI** (`apps/web/src/lib/local.svelte.ts`):

- Settings → Connection: Auto / Server only / Local only, "Show a notice when switching to local",
  estado (cambios sin subir, última sincronización, "Sync now"). Pasar a Auto/Local sin la cuenta
  en el dispositivo pide la contraseña ahí mismo (login por el local).
- Indicador siempre visible "Server" / "Local" (con los cambios pendientes): en la barra de título
  del desktop y arriba a la derecha en Android (`components/ConnBadge.svelte`).
- Dos tokens en `localStorage`: `mythos.token` (server de casa) y `mythos.localToken` (local).
  `lib/api.ts` usa el del server activo. Logout y "Switch account" cierran los dos.
- Caída: un pedido al de casa que no llega (o 502/503/504) y `GET /api/health` que tampoco →
  pasa al local, repite ese pedido ahí y avisa. En el local mira `/api/local/status` cada 20 s;
  con `remote_reachable` → `POST /api/local/sync {token}` → vuelve al de casa ("Back on the server ·
  synced") y recarga la vista. Sin sesión del remoto (`remote_signed_in: false` o un login hecho
  sin conexión) se queda en el local y pide entrar de nuevo para sincronizar.
- Cada sesión de video queda atada al server (y token) donde se abrió: `direct_url`, HLS,
  subtítulos internos y `/status` siguen yendo ahí aunque la API cambie. Si ese video se traba y
  la API ya está en otro server, el reproductor lo reabre en el activo en el mismo segundo.
- Sin ffmpeg (`transcode_available: false`): si `direct_ok` es `false` se prueba solo el
  siguiente stream (hasta 3); si el `<video>` no lo decodifica, lo mismo en vez de degradar.

## Lo que falta

- Android: *foreground service* mientras se reproduce o baja algo en el local (con la pantalla
  apagada Android puede congelar el proceso).
- `caps` reales del WebView de Android para `/api/streams` (hoy usa la misma detección que la
  web, que ya es la del dispositivo).

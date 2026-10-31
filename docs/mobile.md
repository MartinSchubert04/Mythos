# Mythos para Android

La app de Android (`apps/mobile`): un shell de Tauri 2 con la misma interfaz de `apps/web`, lo
nativo que hace falta en el celular y actualizaciones solas, igual que en Haze.

## Dónde va cada cosa

```
apps/mobile/
  package.json        CLI de Tauri y scripts (npm run build:emulator, build:debug, build)
  plugin/             plugin `mythos` (Rust + Kotlin): lo nativo y las actualizaciones
    src/lib.rs        comandos para la interfaz, permisos de la interfaz del server
    src/updater.rs    buscar / bajar / instalar (igual que Haze)
    android/          Kotlin: MythosPlugin.kt (pantalla completa, atras, pantalla encendida,
                      barras, red local) y Updater.kt (GitHub Releases + PackageInstaller)
  src-tauri/          la app: tauri.conf.json (carga apps/web/dist), capabilities, iconos
    src/lib.rs        solo el Builder (plugins mythos y mythos-local)
    gen/android/      proyecto de Gradle (de `tauri android init`, con firma y MainActivity propios)
apps/web/src/lib/
  platform.ts         isTauri / isAndroid / isDesktop, serverBase(), setActiveServer(), pantalla completa
  shell.svelte.ts     todo lo de Android del lado de la interfaz
```

## La dirección del server

El repo y los Releases son públicos: ni el APK ni el instalador de escritorio llevan la dirección
del tailnet. La primera vez el login muestra **Server URL** (sin `https://` se le agrega), se
valida con `GET /api/health` y se guarda. Después se cambia en Settings → Account → Server.
`VITE_DEFAULT_SERVER` (`apps/web/.env.local`) queda solo para probar en la PC; los workflows lo
dejan vacío a propósito.

Android 17 corta las conexiones a la red local sin el permiso `ACCESS_LOCAL_NETWORK` ("dispositivos
cercanos"). Si el server no contesta, la app lo pide y prueba de nuevo (un server con IP de la casa,
o el emulador hablando con la PC en `10.0.2.2`).

## De dónde sale la interfaz

**Por defecto, del APK.** La interfaz es el build de `apps/web` que viene adentro (la página
local, `http://tauri.localhost`), habla con el server por HTTP (`serverBase()`) y se actualiza con
el APK. Es la que va a usar el modo local (el server embebido, en `http://127.0.0.1:<puerto>`):
`setActiveServer(url)` de `platform.ts` cambia el server en caliente sin recargar.

**Opcional: la interfaz del server** (Settings → App → "Load the interface from the server",
apagado por defecto). Con eso prendido, al abrir la app la página local se fija si el server
contesta y trae `<meta name="mythos-shell">` (un server viejo no) y navega a la interfaz que sirve
el server: cada deploy la actualiza al instante, sin reinstalar, y siempre es de la misma versión
que la API. Si el server no contesta, queda la local.

| | Interfaz del APK (por defecto) | Interfaz del server (opcional) |
|---|---|---|
| Actualizar la interfaz | con el APK (solo, de fondo) | con cada deploy, al instante |
| Sin server / modo local | anda (reconexión, o el server embebido) | cae a la del APK |
| Arranque | inmediato, todo local | un pedido de más al server antes de mostrar |
| Permisos nativos | la página local (`capabilities/default.json`) | solo ese origen exacto, en tiempo de ejecución |

Seguridad de la interfaz del server: `open_server` (solo la página local puede llamarlo) le da al
origen exacto que eligió el usuario (no `*.ts.net`) una capability en tiempo de ejecución con los
comandos de `REMOTE_PERMISSIONS` (`plugin/src/lib.rs`): nunca `open_server`, y nada de archivos ni
ventanas. Cualquier otra página que se abriera en el WebView no tiene IPC. Las dos páginas tienen
almacenamiento separado: al entrar a la del server se le pasa el token una sola vez (`handoff`), y
"Change server" o apagar la opción vuelven a la local (`leave_server`).

Contrato entre la interfaz y el APK: los comandos del plugin solo se agregan, nunca se cambian ni se
sacan, y la interfaz tolera que falte uno (un APK más viejo que la interfaz del server).

## Modo local

El server embebido (`docs/local-mode.md`) es el plugin `mythos-local` (`apps/local-server`), el
mismo del desktop: arranca en segundo plano según el modo (Auto / Server only / Local only, en
Settings → Connection) con la carpeta privada de la app, y la interfaz le pregunta dónde está
(`info`). La interfaz cambia de server en caliente con `useLocal()` de `lib/api.ts` (que usa
`setActiveServer(url)` de `platform.ts`); ver `lib/local.svelte.ts`.

- **Red:** el release solo permite HTTPS, salvo `127.0.0.1` y `localhost`
  (`gen/android/app/src/main/res/xml/network_security_config.xml`); el debug, HTTP a cualquier lado
  (`src/debug/res/xml/`). El loopback no necesita el permiso de red local de Android 17.
- **TLS:** Gradle trae `org.rustls:rustls-platform-verifier` (repositorio maven de rustls en
  `gen/android/build.gradle.kts`); el plugin lo inicializa al arrancar.
- **Logs:** `adb logcat -s mythos` (los del server embebido).
- La interfaz del server (opcional) no sirve en modo local: con el server caído la app queda en la
  del APK, que es la que habla con el embebido.

## En el celular

- **Atrás:** el mismo camino que Esc (cierra menús, cajones, diálogos y el tráiler); en el
  reproductor sale de una (no pasa por "salir de pantalla completa"); sin nada abierto vuelve a la
  vista anterior, y en el inicio manda la app al fondo.
- **Reproductor:** al entrar, pantalla completa nativa (sin barras del sistema) y horizontal (gira
  entre las dos). El botón de pantalla completa la saca (se puede girar libre) y la vuelve a poner.
  Al salir, todo normal.
- **Pantalla encendida** mientras un `<video>` se reproduce.
- **De borde a borde** con `env(safe-area-inset-*)` (App.svelte); los iconos de las barras del
  sistema siguen al tema (claros u oscuros).
- **ClientCaps:** la misma detección de la web. El WebView de Android dice h264, hevc, av1, aac,
  opus, flac y `mkv: maybe`; no ac3/eac3/dts (el server convierte el audio). Si el directo falla,
  el reproductor degrada y pasa a HLS (hls.js con MSE).
- Debajo de 700 px de ancho, la barra de pestañas abajo (DESIGN.md §8); en horizontal, la lateral.

## Actualizaciones del APK

Igual que Haze: al rato de abrir y cada seis horas se fija si el último Release de
`MartinSchubert04/Mythos` trae una versión más nueva en `latest-android.json`, la baja de fondo y
verifica el sha256. La firma la verifica Android: un APK firmado con otra clave no se instala encima.

Se instala sola al salir de la app sin nada reproduciéndose, si Android deja hacerlo sin preguntar:
Android 12 o más nuevo, con "Instalar apps desconocidas" y Mythos como quien instaló la versión
actual (`setRequireUserAction(USER_ACTION_NOT_REQUIRED)`, que además necesita el permiso
`UPDATE_PACKAGES_WITHOUT_USER_ACTION`; sin él Android pide confirmar igual). Si no, queda "Update
now" en Settings → App: la primera vez abre el ajuste del permiso y después el diálogo del sistema.
Al abrir la versión nueva la interfaz vuelve a la misma pantalla (y el reproductor al mismo
segundo, con `?resume=1`). El APK bajado se borra al arrancar la siguiente vez.

Probado en el emulador (Android 17): buscar, bajar y verificar desde un `latest-android.json` de
prueba; "Update now" con el permiso y el diálogo; la instalación sola al salir (0.2.1 → 0.2.2 sin
diálogo) y volver a la misma pantalla.

Para probar sin publicar (solo en builds de debug): servir `latest-android.json` y el APK en la PC
y apuntar la app ahí:

```
adb shell run-as dev.mrtn.mythos sh -c 'echo http://10.0.2.2:8000/latest-android.json > files/update-url.txt'
```

## Compilar

Hace falta el SDK y el NDK de Android, Java 21 y los targets de Rust
(`rustup target add aarch64-linux-android x86_64-linux-android`). La CLI de Tauri lee:

```
ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk
NDK_HOME=%ANDROID_HOME%\ndk\28.0.13004108
JAVA_HOME=C:\Program Files\Java\jdk-21
```

```
cd apps/mobile
npm run build:emulator     # APK de debug x86_64 para el emulador
npm run build:debug        # APK de debug arm64 para un celular
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

En Windows `tauri android build` necesita el modo de desarrollador (crea un enlace simbólico del
`.so` en `jniLibs`). Sin eso falla después de compilar Rust; se puede seguir a mano: copiar
`src-tauri/target/x86_64-linux-android/debug/libmythos_mobile_lib.so` a
`gen/android/app/src/main/jniLibs/x86_64/` y correr en `gen/android`
`gradlew assembleUniversalDebug -PabiList=x86_64 -ParchList=x86_64 -PtargetList=x86_64 -x rustBuildUniversalDebug -x rustBuildX86_64Debug`.
El CI corre en Linux y no tiene el problema.

Los registros nativos salen en `adb logcat` (filtrar por `mythos` o `Tauri`). En debug el WebView
se inspecciona desde `chrome://inspect`.

## Publicar

Escritorio y Android salen juntos con la misma versión: el tag `vX.Y.Z` dispara `desktop.yml` y
`android.yml`, y los dos suben al mismo Release con `.github/scripts/release.sh` (el primero lo
crea, marcado como último). Android sube `Mythos-X.Y.Z-android.apk` y `latest-android.json`.

La firma sale de cuatro secrets del repo (Settings → Secrets and variables → Actions). Para
crearlos, una sola vez:

```
keytool -genkeypair -keystore mythos.jks -alias mythos -keyalg RSA -keysize 4096 -validity 36500
base64 -w0 mythos.jks   # en Windows: certutil -encode mythos.jks mythos.b64 (sin las lineas BEGIN/END)
```

| Secret | Valor |
|---|---|
| `ANDROID_KEYSTORE` | `mythos.jks` en base64 |
| `ANDROID_KEYSTORE_PASSWORD` | la contraseña de la keystore |
| `ANDROID_KEY_ALIAS` | `mythos` |
| `ANDROID_KEY_PASSWORD` | la de la clave (si es la misma, se puede omitir) |

Todas las versiones se firman con la misma keystore. Si se pierde, nadie puede actualizar sin
desinstalar: guardala en dos lugares, fuera del repo. Sin `keystore.properties` el release local
sale firmado con la clave de debug (sirve para probar, no para publicar; el CI falla si faltan los
secrets). Para un APK firmado en la PC, crear `apps/mobile/src-tauri/gen/android/keystore.properties`
(está en el `.gitignore`) con `storeFile`, `storePassword`, `keyAlias` y `keyPassword`.

Los workflows (`.github/`) necesitan un token con el scope `workflow` para pushearse.

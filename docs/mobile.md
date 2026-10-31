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
                      barras, red local, `device`: TV y memoria) y Updater.kt (GitHub Releases +
                      PackageInstaller)
  src-tauri/          la app: tauri.conf.json (carga apps/web/dist), capabilities, iconos
    src/lib.rs        solo el Builder (plugins mythos y mythos-local)
    gen/android/      proyecto de Gradle (de `tauri android init`, con firma y MainActivity propios)
apps/web/src/lib/
  platform.ts         isTauri / isAndroid / isDesktop, serverBase(), setActiveServer(), pantalla completa
  servers.svelte.ts   varias direcciones del mismo server: cual usar, latencia, Settings → Server
  shell.svelte.ts     todo lo de Android del lado de la interfaz
  tv.svelte.ts        modo TV: deteccion, escala de 10 pies (viewport de 960 px)
  spatial.svelte.ts   navegacion con el control remoto (flechas, centro, menu)
  profiles.svelte.ts  perfiles de la TV ("Who's watching?", views/Profiles.svelte)
```

## La dirección del server

El repo y los Releases son públicos: ni el APK ni el instalador de escritorio llevan la dirección
del tailnet. La primera vez el login muestra **Server URL** (sin `https://` se le agrega), se
valida con `GET /api/health` y se guarda. Después se cambia en Settings → Account → Server.
Puede tener **varias direcciones del mismo server** (p. ej. Tailscale y la red de casa, `http://` con
`MYTHOS_EXTRA_BIND`): se cargan en el login ("Add another address") o en Settings, se comprueba
que sean el mismo server (`server_id` de `/api/health`) y la app usa la preferida si contesta, si no
la más rápida, y cambia sola al cambiar de red. Ver `docs/local-mode.md`, "Varias direcciones del
server".
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

- **Red:** el release permite HTTP (`gen/android/app/src/main/res/xml/network_security_config.xml`)
  para entrar al server por la red de casa (`http://<ip-del-server>:7700`, con `MYTHOS_EXTRA_BIND`
  en el server; útil en un TV box sin Tailscale) y para el server embebido en `127.0.0.1`. Android no
  deja limitarlo a un rango de IP. Desde afuera de casa va por HTTPS (`tailscale serve`); una
  dirección sin `http://` se completa con `https://`. El loopback no necesita el permiso de red
  local de Android 17; la IP de casa sí (se pide al conectar).
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

## TV (Android TV y TV box)

El mismo APK sirve para un TV box o un Android TV: aparece en el launcher de TV con su banner
(`res/drawable-xhdpi/banner.png`, 320x180, hecho con el logo) y se maneja con el control remoto.
En el celular no cambia nada.

**Instalar en un TV box** (p. ej. un X96 Mini, que no tiene Play Store ni ADB a mano):

- Con la app **Downloader** (o el navegador del box): abrir el link del APK del ultimo Release
  (`Mythos-X.Y.Z-android.apk` en GitHub) e instalar. Hay que permitirle a Downloader "instalar
  apps desconocidas".
- O con un **pendrive**: copiar el APK, abrirlo desde el administrador de archivos del box e
  instalar.
- Despues se actualiza sola como en el celular (Settings → App).

**32 y 64 bits.** Muchos TV box (Allwinner H313, Amlogic S905W...) tienen procesador de 64 bits
pero Android de 32 (userspace `armeabi-v7a`). El APK es universal: trae `lib/arm64-v8a` y
`lib/armeabi-v7a` (librqbit, rusqlite, ring/rustls compilan igual para armv7) y Android elige la
que corresponde. Un solo APK, mismo `versionCode` (lo arma Tauri de la version) y la misma firma:
las apps instaladas se siguen actualizando solas. Pesa unos MB mas que el de solo 64 bits.

**Que es "TV".** El plugin (`MythosPlugin.kt`, comando `device`) dice que es TV si el sistema
esta en modo TV (`UiModeManager`), si tiene `android.software.leanback` o si no tiene pantalla
tactil. Con eso la interfaz (`lib/tv.svelte.ts`) pone `data-tv` en `<html>`; queda guardado y al
abrir de nuevo se aplica antes de pintar (script de `index.html`). En la app es automatico, sin
opcion.

**En el navegador (p. ej. Firefox en el TV box).** Si el WebView del box no se puede actualizar,
la alternativa es la interfaz web del server en Firefox para Android (motor propio, no usa el
WebView; anda en Android 8+ y en 32 bits) por Tailscale. Ahi el modo TV es el mismo y se decide
asi: lo elegido en Settings → Appearance → "TV mode (remote control)" (guardado en ese
navegador), o `?tv=1` / `?tv=0` / `?tv=auto` en la URL (tambien queda guardado). Sin eleccion,
automatico: Android sin pantalla tactil (`maxTouchPoints` 0 y sin puntero "coarse") o un
navegador de Smart TV (Android TV, Fire TV, BRAVIA, Tizen, webOS...). Con el modo TV el login
tambien pide la sesion que no vence y la cuenta queda como perfil, igual que en la app. Lo que
cambia: el boton atras del control navega el historial del navegador; el router
(`setBackGuard`) le pregunta antes a `lib/spatial.svelte.ts`, que con algo abierto (cajon,
dialogo, menu, menu del reproductor, "Who's watching?", "Add profile") lo cierra como Esc y deja
la ruta como estaba; si no hay nada abierto es el atras de siempre (sale del reproductor, vuelve
a la vista anterior). Tambien se aceptan las teclas `BrowserBack` / `GoBack`. Esc del teclado
hace lo mismo que atras.

**Solo server.** En un TV, o con menos de 2 GB de RAM (`MemTotal` < 1,5 GiB o
`isLowRamDevice()`), el server embebido del modo local no arranca nunca (no da para bajar
torrents ahi): el modo es siempre "Server only" y Settings → Connection lo explica en vez de
ofrecer Auto / Local only (`docs/local-mode.md`). El login va directo al server de casa.

**Escala de 10 pies.** La pagina se arma en 960 px de ancho (el lienzo de 960x540 dp de Android TV)
con `<meta name="viewport" content="width=960">`, y el WebView la agranda hasta llenar la
pantalla (el plugin prende `useWideViewPort` y `loadWithOverviewMode` en TV). En un TV 1080p todo
se dibuja al doble, en cualquier densidad que tenga el box. La barra lateral queda en iconos y se
abre encima del contenido cuando el foco entra en ella.

**Liviano.** Los TV box tienen poca memoria y GPU chica (el X96 Mini real: Hi3798MV310, Android 9,
userspace de 32 bits, 1 GB de RAM con ~380 MB libres, Mali-450). En TV no corre el server
embebido (arriba), no se precarga nada grande (hls.js y cada vista se bajan recien al usarse) y
`styles/tv.css` saca todos los `backdrop-filter` (el desenfoque detras de paneles, menus y los
controles del reproductor se recalcula en cada cuadro y hace saltar el video): quedan los fondos
de cada tema.

**Control remoto** (`lib/spatial.svelte.ts`, solo en TV):

- Flechas: el foco va a lo que se puede apretar mas cercano para ese lado, mirando donde esta
  dibujado (filas de posters, hero, barra lateral, detalle, streams, Settings, menus, dialogos).
  A los costados solo se va a lo que esta a la misma altura; arriba/abajo no salta entre la barra
  lateral y el contenido; si no hay nada, la pagina se desplaza (y las filas que cargan al
  acercarse aparecen). La fila se corre sola para que lo enfocado se vea.
- Centro (Enter): aprieta lo enfocado. En un campo de texto abre el teclado del sistema (pasar por
  encima no lo abre: `inputmode="none"` mientras se navega).
- Atras: el Esc de siempre (cierra menus y dialogos, sale del reproductor, vuelve a la vista
  anterior) y al volver el foco queda en la tarjeta de donde se salio. Al cerrar un cajon el foco
  vuelve a lo que lo abrio; si lo enfocado desaparece (un boton que se deshabilita mientras
  guarda), la proxima flecha sigue desde donde estaba.
- Menu del control: el menu contextual del elemento (el de clic derecho / mantener apretado).
- Foco: anillo de 3 px del acento separado del borde, y las tarjetas crecen un poco (con "reducir
  animaciones", sin animar).
- Reproductor: centro = play/pausa; izquierda/derecha = -10/+10 s (manteniendo, el salto crece:
  10, 30 y 60 s por paso, y se aplica una sola vez al soltar); arriba/abajo muestran los controles
  y se mueven entre ellos (subtitulos, audio, velocidad, streams); abajo de todo los esconde; atras
  sale. Las teclas multimedia del control (play/pausa, adelantar, retroceder) tambien andan.

**Perfiles.** En la TV cada cuenta queda guardada como un perfil (`lib/profiles.svelte.ts`): se
entra una sola vez con la contraseña y la sesion no vence (`persistent`, `docs/API.md`). Al abrir,
con mas de un perfil, "Who's watching?" (el ultimo usado ya elegido); el avatar de la barra lateral
lo vuelve a abrir (atras lo cierra sin cambiar de perfil). "Add profile" suma otra cuenta; "Manage
profiles" abre cada perfil para elegirle el avatar (con la sesion de ese perfil, sin entrar) o
quitarlo (cierra su sesion en el server). Si alguien revoca esa sesion desde Settings → Signed-in
devices o el admin desde Admin → Users → "Signed-in devices", el perfil se borra solo y vuelve el
selector. Cambiar de perfil recarga la pagina (ninguna cache queda de la cuenta anterior). Solo
en TV: en el celular y la PC "Switch account" sigue siendo salir y entrar.

**WebView minimo: Chromium 111.** La interfaz usa `color-mix()` en casi todos los colores (111),
`:has()` (105) y `AbortSignal.timeout` (103). Muchos TV box traen un Android System WebView viejo
(el X96 Mini, 66 de 2018): con eso el modulo de la app ni compila. Por eso `index.html` tiene un
script ES5 que mira la version de Chrome/Chromium del userAgent antes de cargar nada: si es menor
a 111 no carga la app (el plugin `gate` de `vite.config.ts` saca el `<script type="module">` del
HTML y lo carga `mythosBoot` solo si alcanza) y muestra "Update Android System WebView" con los
pasos, las dos versiones y, como alternativa, abrir la direccion del server en Firefox (ver "En el
navegador"). En un navegador de PC viejo basado en Chromium dice "Update your browser"; Firefox
pide la 121 (`:has()`; si no, "Update Firefox") y Safari pasa siempre. Para verla en la PC:
`http://localhost:5173/?webviewTest=66`.

Como actualizar el WebView de un TV box: desde Google Play ("Android System WebView" → Update) o,
sin Play Store, instalando el APK de Android System WebView de la ABI del box (armeabi-v7a o
arm64-v8a) con Downloader o un pendrive y eligiendolo en Developer options → WebView
implementation. Ojo: algunos firmwares traen un WebView de AOSP (`com.android.webview`) firmado
por el fabricante que no se puede reemplazar sin root (y no muestran "WebView implementation");
ahi la app no anda y queda Firefox. Ojo tambien con el minSdk del APK del WebView: un box con
Android 8-9 (API 26-28, aunque diga 10 o 13) no instala uno que pida API 29.

**Android del APK.** `minSdk` 24 (Android 7.0): anda en los box de Android 8-9. Lo que solo
existe en versiones nuevas del plugin tiene su chequeo (`canRequestPackageInstalls` desde 26,
`setRequireUserAction` y `FLAG_MUTABLE` desde 31, `ACCESS_LOCAL_NETWORK` desde 37); los permisos
que el sistema no conoce se ignoran.

**Probar en la PC** (sin el box): `npm run dev:mock` y `http://localhost:5173/?tv=1` (o el switch
de Settings → Appearance; `?tv=auto` vuelve a automatico), con flechas, Enter y Esc (Esc hace de
atras). El navegador de escritorio ignora el ancho del meta viewport: para ver la escala, una
ventana (o un iframe) de 960x540.

Sin probar en el aparato real todavia: la deteccion (`device`) y el viewport de 960 px dependen del
WebView del box.

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
(`rustup target add aarch64-linux-android armv7-linux-androideabi x86_64-linux-android`). La CLI
de Tauri lee:

```
ANDROID_HOME=%LOCALAPPDATA%\Android\Sdk
NDK_HOME=%ANDROID_HOME%\ndk\28.0.13004108
JAVA_HOME=C:\Program Files\Java\jdk-21
```

```
cd apps/mobile
npm run build:emulator     # APK de debug x86_64 para el emulador
npm run build:debug        # APK de debug arm64 + armv7 (celular o TV box)
npm run build              # APK de release arm64 + armv7 (el que se publica)
adb install -r src-tauri/gen/android/app/build/outputs/apk/universal/debug/app-universal-debug.apk
```

En Windows `tauri android build` necesita el modo de desarrollador (crea un enlace simbólico del
`.so` en `jniLibs`). Sin eso falla después de compilar Rust; se puede seguir a mano: copiar
`src-tauri/target/x86_64-linux-android/debug/libmythos_mobile_lib.so` a
`gen/android/app/src/main/jniLibs/x86_64/` y correr en `gen/android`
`gradlew assembleUniversalDebug -PabiList=x86_64 -ParchList=x86_64 -PtargetList=x86_64 -x rustBuildUniversalDebug -x rustBuildX86_64Debug`.
Con dos ABIs (el release), lo mismo para cada una:

```
copy src-tauri\target\aarch64-linux-android\release\libmythos_mobile_lib.so gen\android\app\src\main\jniLibs\arm64-v8a\
copy src-tauri\target\armv7-linux-androideabi\release\libmythos_mobile_lib.so gen\android\app\src\main\jniLibs\armeabi-v7a\
cd src-tauri\gen\android
gradlew.bat assembleUniversalRelease -PabiList=arm64-v8a,armeabi-v7a -ParchList=arm64,arm -PtargetList=aarch64,armv7 -x rustBuildUniversalRelease -x rustBuildArm64Release -x rustBuildArmRelease
```

El CI corre en Linux y no tiene el problema (compila las dos y verifica que el APK las traiga).

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

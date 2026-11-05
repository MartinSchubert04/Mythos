# Mythos para Samsung (Tizen)

La app de las teles Samsung (`apps/tizen`): una app web de Tizen (`.wgt`) mínima que hace de shell,
como la de Android con "interfaz del server". Pide la dirección del server la primera vez, la
guarda, comprueba que sea un Mythos y navega a la interfaz que sirve el server, que entra sola en
modo TV (`lib/tv.svelte.ts` reconoce "Tizen" en el user agent).

## Dónde va cada cosa

```
apps/tizen/
  config.xml          la app: id MythosTV01.Mythos (el paquete tiene que tener 10 caracteres),
                      version (la del repo), privilegios, navegar a cualquier origen
  index.html          la pagina local: elegir server ("Conectando a ..." / "Cambiar de servidor"; en
                      español, o en inglés si la tele está en inglés)
  shell.js, shell.css (ES5: en una tele vieja dice que no alcanza en vez de quedar en negro)
  icon.png            el de la app de Android (512x512)
  build.ps1           arma build\Mythos.wgt firmado y, con -Tv <ip>, lo instala y lo abre
  tools/samsung-cert.py  los certificados de Samsung (abajo)
apps/web/src/lib/tizen.ts   lo de Samsung en la interfaz: nombres de teclas, teclas multimedia,
                            salir de la app
```

## Cómo funciona

- **La dirección del server.** Igual que en Android, ningún build la lleva. La tele no tiene
  Tailscale: va la de la red de casa (`http://<ip-del-server>:7700`, con `MYTHOS_EXTRA_BIND` en el
  server). Se escribe con el teclado de la tele (el centro del control lo abre; sin `http://` ni
  puerto completa `http://` y `:7700`) o se busca con **Buscar en esta red** ("Find on this network"): prueba
  `http://<x.y.z.1-254>:7700/icon.svg` en la red de la tele (la IP sale de `tizen.systeminfo`).
  Queda guardada; al abrir se ve "Conectando a ..." un momento con **Cambiar de servidor**.
- **Comprobar que es un Mythos.** `GET /api/health` (`ok`, y que no sea el server embebido de
  otra app). Si la página local no lo puede leer (CORS: el server solo deja leer a sus orígenes
  conocidos), alcanza con que cargue `/icon.svg` de esa dirección.
- **Navegar, no iframe.** El server manda `frame-ancestors 'none'`, así que la página local navega
  a la del server (`<tizen:allow-navigation>*`: la dirección la elige el usuario). Antes de irse
  marca su entrada del historial (`#away`).
- **Teclas.** `lib/tizen.ts` traduce los códigos de Samsung a los nombres estándar que ya usan
  `lib/spatial.svelte.ts` y el reproductor: atrás (10009) = `BrowserBack`, y las multimedia
  (play/pausa, play, pausa, stop, adelantar, retroceder). Las multimedia solo llegan si se
  registran con `tizen.tvinputdevice.registerKey`: lo hacen la página local antes de navegar y la
  interfaz al arrancar (si esa página tiene `tizen`). Stop pausa.
- **Atrás** (`tizenBack` en `lib/spatial.svelte.ts`). El control manda `XF86Back` (keyCode 10009)
  y la tele no navega sola. Igual que el botón atrás de Android: primero un Esc a lo enfocado
  (cierra cajón, menú, diálogo, tráiler, menús del reproductor; en el reproductor sin nada abierto,
  sale); si nadie lo usa, la vista anterior. En el inicio: si se estaba recorriendo las filas,
  vuelve arriba (al Play del destacado); arriba de todo, cierra la app sin preguntar (el paso
  "volver arriba" ya evita salir sin querer, y una confirmación sería una tecla más cada vez).
  Cerrar: con `tizen.application` si la página del server lo tiene; si no, `history.back()` vuelve a
  la página local, que al verse con `#away` se cierra. Probado con el control real en inicio,
  ficha, cajón de streams, menú ⋯, tráiler, reproductor (con y sin menú) y salida.

## Reproducir: AC-3 / E-AC-3

La tele acepta AC-3 y E-AC-3 solos en MSE pero **no mezclados con el video** en el mismo
SourceBuffer (`video/mp4;codecs="hvc1…,ec-3"` → no soportado), y los segmentos HLS del server llevan
las dos pistas juntas. Antes la interfaz decía que sí (`ClientCaps`), el server copiaba el audio y
hls.js no podía ni crear el buffer: "The stream stopped responding" con casi cualquier release
WEB-DL (DDP5.1). Ahora `lib/caps.ts` pide también la combinación mezclada (el server convierte el
audio) y, si igual pasa (`bufferAddCodecError`), el reproductor degrada el audio y sigue.

## Rendimiento

La página se dibuja al doble (960 px en 1920) y la UN50DU7000 rasteriza por CPU: lo que cuesta es
redibujar, no el JavaScript. En TV (`data-tv`):

- Siempre modo liviano (`data-lite`, `lib/perf.ts`): sin desenfoques, sin el zoom lento del hero,
  sin la aparición escalonada de las tarjetas, fondos de 1280 px como máximo.
- Desplazamiento de un salto (`scrollBehavior()` de `lib/motion.ts`, `styles/tv.css`): el suave
  redibujaba en cada cuadro y era la mayoría de los cuadros perdidos.
- Las tarjetas crecen con el foco sin animar y con solo el anillo (sin sombra difusa): animado,
  cada tarjeta pasaba a ser una capa mientras duraba y al terminar se redibujaba la página debajo.
- El hero rota solo con el control quieto 8 s; las filas no miden sus flechas (no se muestran);
  a los costados la navegación busca solo en la fila (`lib/spatial.svelte.ts`).

Medido en la tele (inicio con las filas ya cargadas, 25 flechas): antes ~30 cuadros/s, 70–82
cuadros de más de 50 ms, tecla→cuadro p90 390–440 ms, 62–67 tareas largas; después ~55 cuadros/s,
16–18, 234–241 ms y 13. El raster en reposo/navegando bajó de ~128 % a ~46 % de un núcleo.

## Herramientas (una vez, en la PC)

- **Tizen Studio CLI** (`web-cli_Tizen_Studio_6.1_windows-64.exe` de
  `https://download.tizen.org/sdk/Installer/tizen-studio_6.1/`). El instalador pide administrador;
  sin admin se instala en el perfil:
  `$env:__COMPAT_LAYER='RunAsInvoker'; .\web-cli.exe --accept-license $env:USERPROFILE\tizen-studio`
  Trae `tools\sdb.exe`, `tools\ide\bin\tizen.bat` y un Java 8 propio (`jdk\`).
  Ojo: el package manager (`package-manager-cli.exe install ...`) intenta actualizar WebCLI
  primero, falla ("Cannot install updates") y deja borrados el `jdk\` y parte del package manager:
  no hace falta para nada de esto (si se usó, reinstalar).
- **Python 3** con `cryptography` y `requests` (para `samsung-cert.py`).

## Modo desarrollador de la tele

Una tele de fábrica no instala un `.wgt` desde un pendrive ni de cualquier lado: solo desde la
tienda de Samsung o, en modo desarrollador, por `sdb` desde la PC elegida y firmado con un
certificado de Samsung que nombra a esa tele.

1. En la tele: Apps → apretar `1 2 3 4 5` → Developer mode **On** y **Host PC IP** = la IP de la PC.
2. Reiniciar la tele de verdad (mantener apretado el botón de encendido hasta que aparezca el logo):
   recién ahí abre el puerto de `sdb` (26101).
3. `sdb connect <ip-de-la-tele>` y `sdb -s <ip-de-la-tele>:26101 shell 0 getduid` (el DUID).

## Certificados de Samsung

`apps/tizen/tools/samsung-cert.py` hace lo mismo que el Certificate Manager de Tizen Studio con la
Samsung Certificate Extension, sin Eclipse: abre el login de la cuenta de Samsung en el navegador
(responde a `http://localhost:4794/signin/callback`), pide a Samsung el certificado de autor y el
de distribuidor (con el DUID de la tele, nivel Public), arma los `.p12` con las CA de Samsung y crea
el perfil de firma `Mythos` en Tizen Studio:

```
python apps\tizen\tools\samsung-cert.py --duid <DUID> --tizen $env:USERPROFILE\tizen-studio
```

Quedan en `~/.mythos-signing/tizen/` (fuera del repo, como las claves de Android):
`author.p12`, `distributor.p12`, `password.txt` (la de los dos `.p12`) y `ca/` (las CA públicas de
Samsung, bajadas de la extensión). El perfil de firma vive en
`~/tizen-studio-data/profile/profiles.xml`.

- **El autor es para siempre**: firma cada versión, y la tele no instala encima una firmada por
  otro autor (hay que desinstalar). No se pide de nuevo: el script se niega si ya hay un
  `author.p12`.
- **Otra tele**: `--reuse-author --duid <DUID-1> --duid <DUID-2>` pide solo un distribuidor nuevo
  con todas las teles.

## Compilar, firmar e instalar

```
cd apps/tizen
.\build.ps1                       # build\Mythos.wgt firmado con el perfil "Mythos"
.\build.ps1 -Tv 192.168.0.139     # además: sdb connect, tizen install y abrirla
```

`build.ps1` copia solo lo de la app a `build\` (la firma escribe ahí) y corre
`tizen package -t wgt -s Mythos`, `tizen install` y `tizen run -p MythosTV01.Mythos`. Para
depurar: `sdb -s <ip>:26101 shell 0 debug MythosTV01.Mythos` da un puerto del inspector
(`sdb forward tcp:<puerto> tcp:<puerto>` y `chrome://inspect`).

## Actualizar

- **La interfaz** viene del server: cada deploy la actualiza sola, sin tocar la tele.
- **La app** (la página local, `config.xml`) casi nunca cambia: se actualiza reinstalando con
  `build.ps1 -Tv <ip>` (mismo autor; la dirección guardada queda). No hay actualización sola.

## Limitaciones

- Hace falta el modo desarrollador con esta PC como Host PC y el certificado de distribuidor con el
  DUID de la tele (para instalar y para reinstalar cada versión).
- **No va a los Releases**: un `.wgt` firmado sirve solo para las teles de su certificado de
  distribuidor, y no se puede instalar sin `sdb` (ni por pendrive). Por eso no hay workflow de CI:
  armar un `.wgt` en CI no le sirve a nadie sin el certificado de su tele. Publicarla para cualquier
  tele sería pasar por la tienda de Samsung (Seller Office).
- La interfaz pide Chromium 111 (como en Android): teles Samsung con Tizen 8 o más nuevo (2024 en
  adelante; Tizen 7 trae Chromium 94). En una más vieja la página local lo dice.
- Solo server: la tele no corre el server embebido del modo local.

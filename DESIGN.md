# DESIGN.md

Sistema de diseño de Mythos. Formato: [awesome-design-md](https://github.com/VoltAgent/awesome-design-md)
(nueve secciones). Portado de Ethereon (mismos temas, paletas, fuentes, movimiento y componentes),
adaptado a una app de películas. Leer antes de tocar la interfaz (`apps/web`).

## 1. Tema visual y atmósfera

Un cliente tipo Stremio para uso personal: las imágenes mandan. Pósters en filas y grillas, un hero
grande con el fondo de algo destacado, el detalle con el fondo de la película, y el reproductor como
protagonista absoluto (negro, controles que se esconden solos). Se tiene que sentir premium y fluida:
cada carga tiene esqueleto con brillo, cada imagen aparece en fundido, nada salta de lugar.

- **Densidad:** media. Pósters de 118 a 168 px de ancho según la ventana; tarjetas de "Continue
  watching" apaisadas de 230 a 320 px.
- **Voz:** literal y en inglés. "Play", "Resume from 42:10", "Choose stream", "Try another stream",
  "Buffering 72%…". Sin metáforas.
- **Seis temas, una estructura.** Cambian colores, bordes, sombras, fondos y fuentes; nunca el layout.
- **La única diferencia entre web y desktop** es la barra de título propia del desktop. En Android
  (`docs/mobile.md`): botón atrás, reproductor en pantalla completa horizontal, de borde a borde con
  `env(safe-area-inset-*)` y Settings → App (versión y actualizaciones).

## 2. Colores y roles

Todos los colores son variables de `apps/web/src/styles/themes.css`. Ningún componente tiene colores
fijos.

| Variable | Rol |
|---|---|
| `--bg`, `--bg-image` | Fondo de la ventana (Warp y Haze usan gradientes, Poolsuite un atardecer) |
| `--sidebar`, `--panel`, `--panel-2` | Superficies, de más atrás a más adelante |
| `--line`, `--line-strong`, `--border` | Bordes y su grosor (2 px en Poolsuite y Athanor) |
| `--text`, `--text-2`, `--text-3` | Texto principal, secundario y terciario |
| `--accent`, `--accent-ink`, `--primary-bg` | Acento, texto sobre el acento, botón principal (puede ser gradiente). También la barra de progreso y el brillo de los pósters |
| `--danger`, `--warn`, `--ok` | Borrar o error; estrellas de IMDb y pocos seeders; muchos seeders, "Instant", en vivo |
| `--media-bg`, `--on-media`, `--on-media-2`, `--media-scrim`, `--media-line` | Comunes a todos los temas: el negro del video y lo que va encima (controles, subtítulos, menús del reproductor) |
| `--win-close` | El rojo de Windows del botón cerrar (desktop) |

Sobre el hero y el detalle el texto usa los colores del tema (`--text`) y el degradado funde la imagen
con `--bg`: en temas claros la imagen queda lavada y el texto oscuro se lee igual. El reproductor es
siempre oscuro (`--media-*`), con el acento del tema en la barra de progreso.

Cada tema tiene paletas (Settings → Appearance). Se definen en `apps/web/scripts/palettes.py`, que
genera `styles/palettes.css` y `lib/palettes.gen.ts` (`npm run palettes`); cada tema recuerda la suya.

| Tema | Paletas |
|---|---|
| Default | Mint, Nord, Dracula, Tokyo Night, Catppuccin, Gruvbox, Paper (clara), Srcery |
| Warp | Violet, Solar Flare, Pink City, Jellyfish, Leafy, Red Rock, Ember |
| Poolsuite FM | Sunset, Mint Pool, Tropicana, Lavender, Beach, Platinum, Monochrome (oscura), Vellum |
| Haze | Cream, Peach, Rose, Mauve, Sky, Sage, Amber |
| Athanor | Srcery, Umber, Cinnabar, Vellum (clara), Orpiment (clara) |
| Neovim | Neovim, Nightshade, Oxblood, Paper (clara), Moss, Kanagawa |

| Tema | Fondo | Acento | Rasgo |
|---|---|---|---|
| Default | `#0d0f14` | menta `#6ee7b7` | Plano, sobrio |
| Warp | negro con luces violeta y cian | gradiente `#8b5cf6 → #22d3ee` | Paneles translúcidos con desenfoque |
| Poolsuite FM | atardecer pastel | rosa `#ff4f9a` | Claro, bordes negros de 2 px, sombras duras, barra rayada del Mac clásico |
| Haze | bruma cálida y fría | crema `#f9e2af` | Vidrio |
| Athanor | `#1C1B19` (Srcery) | ámbar `#FBB829` | Pixel clay: esquinas en escalera, biseles, sombras tramadas; fuente bitmap |
| Neovim | `#0c0c0c` | gris claro | Todo en JetBrains Mono, poco radio |

## 3. Tipografía

Inter Variable para todo y Geist Mono Variable para tiempos, velocidades y tamaños (con
`tabular-nums`). Vienen empaquetadas (`@fontsource`), el navegador baja cada una recién cuando un tema
la usa.

**Excepciones:** Athanor usa PxPlus IBM VGA 8x16 a 16 px exactos (bitmap, sin negrita) y títulos en
Jacquard 24; Neovim usa JetBrains Mono. Los subtítulos del video van siempre en Inter (`--font-sub`).

| Uso | Tamaño | Peso |
|---|---|---|
| Título del hero / detalle (`.display-title`) | 30 a 58 px (clamp) | 760 |
| Título de página | 21 px | 650 |
| Título de fila | 16 px | 650 |
| Texto | 13,5 px (descripciones 14,5 a 15 px) | 400 a 560 |
| Etiquetas de sección | 11,5 px mayúsculas | 650 |
| Mono (tiempos, MB/s) | 12,5 a 13 px | 600 |
| Subtítulos | `clamp(18px, 2.4vw, 40px)` × tamaño elegido | 600 |

## 4. Componentes

- **Póster** (`PosterCard`): 2:3, imagen chica (`/poster/small/`) con `loading="lazy"` y fundido al
  cargar. Al pasar el mouse sube 4 px, crece 4 %, el borde y la sombra toman el acento y un reflejo lo
  cruza; aparece el rating. Al pasar el mouse ya carga el detalle (código y datos). Al hacer clic el
  póster vuela al detalle (View Transitions).
- **Fila** (`Row`): título, scroll horizontal suave con flechas que aparecen al pasar el mouse, carga
  sus datos recién al acercarse a la pantalla, esqueletos mientras tanto. Dibuja las tarjetas de a
  16 (las siguientes al acercarse al final) y fuera de pantalla no se pinta
  (`content-visibility: auto`): el inicio tiene diez filas de ~50 títulos.
- **Hero** (`Hero`): fondo con zoom lento, logo o título, rating, año, géneros, descripción, Play /
  More info. Rota cada 8 s con fundido cruzado (espera a que la imagen siguiente esté bajada).
- **Continue watching** (`ContinueCard`): 16:9 con barra de progreso, episodio y tiempo restante.
- **New episodes** (`NewEpisodeCard`, `lib/newEpisodes.ts`): debajo de Continue watching, un episodio
  por serie de la Library salido en los últimos 14 días y sin empezar ("New · 3" si hay más). Sin
  esqueleto: la fila aparece solo si hay algo. Las metas se piden de a 4 y lo reciente de cada serie
  queda una hora en `sessionStorage`.
- **Lista de streams** (`StreamList`): calidad grande (4K con el acento), tamaño, seeders con color
  (`--ok` ≥ 50, `--warn` ≥ 10, `--danger` menos), fuente, idiomas, tags como chips (HDR, DV, REMUX
  resaltados). Los "cached" llevan "Instant" y borde verde. El primero dice "Best". Con un idioma de
  audio en Settings → Playback, los streams en ese idioma suben (banderas y palabras del release:
  "Latino", "Castellano"…; nunca uno con < 5 seeders o CAM). Menú ⋯ (al pasar el mouse, clic
  derecho o mantener apretado): "Copy stream link" (el link directo con `?t=`: el tooltip avisa que
  lleva la sesión) y, en desktop, "Open in VLC".
- **Menú chico** (`PopMenu`, `lib/press.ts`): flota donde se pidió (clic derecho, mantener apretado
  550 ms o un botón ⋯), colgado de `<body>`; Esc o tocar afuera lo cierra.
- **Reproductor** (`views/Player.svelte`): controles propios que se esconden a los 2,8 s; barra con
  lo bufferizado y el tiempo al pasar el mouse (`SeekBar`); menús de subtítulos, audio y velocidad;
  panel de streams a la derecha; pantalla de carga con datos vivos (`PlayerLoading`); diálogo de
  retomar; panel "Try another stream"; "Next episode" en los créditos y cuenta regresiva al final.
  Sin botones nuevos: mantener Space (o el dedo) = 2× con la pastilla "2×" arriba; `[`/`]` la
  velocidad; V la escala (Fit = entero, Fill = estirado, Crop = llena recortando; se recuerda en la
  sesión); D las estadísticas (modo, códec, peers, MB/s, buffer, cuadros perdidos; `/status` solo
  mientras se ven; en el celular, "Playback stats" en el panel de streams); `?` la lista de atajos.
  Con el dedo: tocar muestra/esconde los controles, doble toque a un costado −10/+10 s con una onda
  (seguir tocando suma), pellizcar afuera = Crop y adentro = Fit. Media Session: título, póster,
  posición y play/pausa/saltos/siguiente episodio en las teclas multimedia, auriculares y la
  notificación de Android.
- **Cajón (Drawer):** formularios y la lista de streams del detalle, a la derecha sobre un velo.
- **Select**, **Switch**, **Seg**, **Confirm**, **Section**: los de Ethereon, iguales.
- **Avisos (toasts):** entran desde abajo a la derecha; rojos si fallan.
- **Tooltips propios** (`lib/tooltip.ts`): todo `title=` se muestra con demora y estilo del tema.
- **Menú de subtítulos** (por pasos, como Stremio): primero "Off" y una entrada por idioma con su
  nombre en ese idioma ("Español (Latinoamérica)", "Português (Brasil)"), la cantidad de opciones y
  "Embedded" si el archivo trae alguna; el preferido y el del audio arriba, después los que vienen
  en el archivo y el resto alfabético. Elegir un idioma pone su primera opción y pasa (de costado)
  a sus opciones: las internas primero, después las de OpenSubtitles ("Version n"). Abajo, fijos:
  **Delay** de la opción elegida (−0,5 / −0,1 / +0,1 / +0,5 s y reset; se recuerda por archivo y
  pista, y la lista marca las que tienen uno guardado), y el aspecto, que es global y viaja con las
  preferencias: **Size** (75 a 200 %), **Position** (altura desde abajo) y **Background** (None,
  Light, Dark, Solid). Con un subtítulo puesto, el menú abre directo en su idioma. En celular es
  una hoja de lado a lado con filas de 44 px. Los subtítulos los dibuja la app (no `<track>` ni
  `::cue`), así se ven igual en Chrome, WebView2 y Android y siguen al tiempo real en HLS.
- **Botones del detalle**: siempre en una fila; en celular "Choose stream" queda solo con el ícono.
- **Cast** (detalle): avatares con iniciales sobre el acento (el catálogo no trae fotos). Debajo,
  la fila "More like this" (mismo género principal).
- **Episodios**: en pantallas anchas van en dos o tres columnas; los especiales (temporada 0) se
  llaman "Specials" y van al final; con más de 6 temporadas, Select en vez de Seg. Visto / no visto a
  mano: el tilde al pasar el mouse por un episodio, clic derecho o mantener apretado (menú), el ⋯
  junto a "Episodes" para toda la temporada, y en las películas el botón del tilde en la fila de
  acciones. Es progreso con la posición al final (o borrarlo). "Hide spoilers" (Settings →
  Playback): imagen desenfocada y descripción tapada en los no empezados hasta pasar el mouse (con
  el dedo, tocar la descripción).

## 4b. Estados

| Estado | Qué se ve |
|---|---|
| Cargando | Esqueletos con brillo en filas, grillas, detalle, admin; splash con la marca al abrir |
| Vacío | Ícono + frase + acción: Library vacía ("Browse"), búsqueda sin resultados, sin "Continue watching" (la fila no aparece) |
| Sin streams | "No streams found" con la sugerencia (muy nuevo, o los límites de Settings) y Retry |
| Server caído | Pastilla arriba "Can't reach the server. Retrying…" con Retry; se reintenta solo cada 5 s y al volver la vista se recarga. Si pasa al abrir: pantalla de reconexión (no el login) |
| Sesión vencida (401) | Login con el aviso "Your session expired" |
| Error al reproducir (502, códec) | Panel en el reproductor con el motivo y "Try another stream" |
| Sin imágenes | Póster tipográfico (degradado del acento + nombre + año); detalle con el póster desenfocado de fondo |

## 5. Layout

Barra lateral a la izquierda (212 px, o 64 px plegada con solo íconos; se recuerda) con Home,
Search, Library, Settings y Admin (solo admins), y el usuario abajo. El contenido scrollea en su
propio contenedor y recuerda dónde estaba al volver.

Desktop: arriba una barra de 40 px (`Titlebar`) que es la barra de título de la ventana sin marco:
plegar la barra lateral, la marca, hueco para arrastrar (doble clic maximiza), buscador y
minimizar / maximizar / cerrar.

El reproductor ocupa toda la ventana (sin barra lateral ni de título); en desktop sus controles de
ventana van en su barra de arriba, que también se arrastra.

Rutas con hash: `#/`, `#/search?q=`, `#/title/{type}/{id}`, `#/watch/{type}/{id}?s={stream}`,
`#/library`, `#/settings`, `#/admin`. Cada vista se carga recién cuando hace falta.

Atajos (listados en Settings → Keyboard shortcuts): `/` o `Ctrl K` buscar (lo que se escribe
mientras la vista carga no se pierde); flechas entre pósters de una fila y entre filas; Esc cierra
paneles y menús. En el reproductor: espacio/K (mantener espacio: 2×), F, M, ←/→ (5 s), J/L (10 s),
↑/↓ volumen, `[`/`]` velocidad, V escala, C subtítulos, G/H el subtítulo puesto 0,5 s antes/después
(con Shift 0,1 s), -/= tamaño de los subtítulos, N siguiente episodio, D estadísticas, `?` atajos, Esc
(cierra menú → sale de pantalla completa → vuelve). La lista vive en `lib/shortcuts.ts` (Settings y
el `?` del reproductor usan la misma).

## 5b. Movimiento

Todas las animaciones pasan por `apps/web/src/lib/motion.ts` (transiciones de Svelte) o son CSS
cortas de `base.css`, y duran 0 con "reducir animaciones" del sistema o de Settings
(`data-reduce-motion` en `<html>`). **Solo `transform` y `opacity`**: nada que recalcule layout en
cada cuadro (la barra del reproductor usa `scaleX`/`translateX`, los puntos del hero `scaleX`, el
volumen flota encima). Sin `filter: blur` animado. Medido con un trace (home → detalle → player →
atrás y scroll de filas): sin tareas largas (> 50 ms) y CLS 0.

| Qué | Cómo |
|---|---|
| Cambiar de vista | sube 6 px y aparece (220 ms) |
| Tarjeta → detalle | el póster vuela (View Transitions, 420 ms); sin soporte, el cambio de vista normal |
| Hero | fundido cruzado de 900 ms y zoom lento del fondo (12 s) |
| Pósters y episodios | aparecen escalonados (30 ms entre uno y otro); al pasar, suben y crecen con resorte |
| Imágenes | fundido de 450 ms al terminar de cargar |
| Cargando | esqueletos con un brillo que los cruza (1,4 s) |
| Cajón | entra desde la derecha con un poco de resorte |
| Diálogos, menús, desplegables | crecen desde 94-95 % (160 ms) |
| Botones | suben 1 px al pasar, se hunden al apretar; el principal tiene un brillo que lo cruza |
| Login | la marca brilla; contraseña incorrecta: la tarjeta tiembla |
| Reproductor | los controles se van hacia afuera; play/pausa/saltos muestran un ícono al centro que crece y se desvanece |
| Avisos | entran desde abajo y se reacomodan (flip) |

En Athanor el movimiento es por pasos (`steps()`), como el pixel art.

**Equipos flojos** (`lib/perf.ts`: ≤ 2 GB de RAM, ≤ 2 núcleos, o Android con ≤ 4 núcleos, como las
TV boxes; se fuerza con `localStorage["mythos.lite"]` = `"1"` o `"0"`): `data-lite` en `<html>`
saca los desenfoques (`--blur: none` y sin `backdrop-filter`), el zoom lento del hero, la
aparición escalonada de las tarjetas y la animación de las sombras; los fondos se piden de 1280 px
como máximo. El resto (fundidos, hover, transiciones de vista) queda igual.

## 6. Profundidad

Default, Warp, Haze y Neovim: sin sombras en reposo; al pasar el mouse los pósters toman una sombra
y un brillo del acento. Poolsuite: sombras duras desplazadas (`3px 3px 0`) y al pasar el mouse el
póster se corre y la sombra crece. Athanor: el póster sube un paso, sin sombra difusa. Lo que flota
(diálogos, menús, cajones) usa `--pop-shadow`. El reproductor usa vidrio oscuro (`backdrop-filter`)
para menús y paneles.

## 7. Qué sí y qué no

- Sí: nunca una pantalla en blanco. Siempre esqueleto, splash con la marca o la pantalla de carga.
- Sí: reservar el tamaño de las imágenes (`aspect-ratio`) para que nada salte al cargar.
- Sí: estados con color y texto a la vez (seeders en rojo y el número; "Instant" verde).
- Sí: secretos nunca visibles; la clave de Real-Debrid guardada muestra "Stored. Type to replace."
- Sí: cada imagen del tamaño en que se muestra (`lib/images.ts`): pósters `small`; fondos y
  miniaturas según el ancho real (fondo de 480 px de metahub, achicado por el server a
  640/960/1280, o el original de 1920 solo si la pantalla lo pide). Ver `docs/API.md`
  (`/api/media/img`).
- No: `backdrop-filter`, `transform: translateZ(0)` ni `will-change` en algo que se repite
  (tarjetas, filas): cada uno es una capa del compositor y con cientos el scroll se traba. El
  desenfoque de una tarjeta va solo al pasar el mouse.
- No: colores fijos en componentes. Si hace falta uno, es una variable nueva en `themes.css`.
- No: animaciones largas fuera del hero y la transición del póster.
- No: librerías de UI. Íconos de `@lucide/svelte` importados de a uno.

## 8. Comportamiento responsive

Ventana mínima del desktop: 900 x 560. Las grillas usan `auto-fill`; las filas, anchos con `clamp`.
Hasta 700 px de ancho (celular): la barra lateral pasa a ser una barra abajo con íconos y nombre, el
detalle apila póster y datos, el reproductor esconde la barra de volumen. En pantallas táctiles no
hay barras de scroll fijas ni foco automático en el login (el teclado taparía la pantalla). Márgenes laterales con
`--gutter` (16 a 44 px).

## 9. Guía para agentes

Antes de cambiar la interfaz: leer este archivo, `styles/themes.css` y `docs/API.md`. Probar en los
seis temas (Settings → Appearance) y con "Reduce motion". `npm run check` sin errores ni avisos y
`npm run build`. Sin server: `npm run dev:mock` (datos de Cinemeta guardados y un video de prueba;
usuarios `admin`/`admin` y `test`/`test`). Comentarios en español, textos de la interfaz en inglés.

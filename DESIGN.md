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
- **La única diferencia entre web y desktop** es la barra de título propia del desktop.

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
  sus datos recién al acercarse a la pantalla, esqueletos mientras tanto.
- **Hero** (`Hero`): fondo con zoom lento, logo o título, rating, año, géneros, descripción, Play /
  More info. Rota cada 8 s con fundido cruzado (espera a que la imagen siguiente esté bajada).
- **Continue watching** (`ContinueCard`): 16:9 con barra de progreso, episodio y tiempo restante.
- **Lista de streams** (`StreamList`): calidad grande (4K con el acento), tamaño, seeders con color
  (`--ok` ≥ 50, `--warn` ≥ 10, `--danger` menos), fuente, idiomas, tags como chips (HDR, DV, REMUX
  resaltados). Los "cached" llevan "Instant" y borde verde. El primero dice "Best".
- **Reproductor** (`views/Player.svelte`): controles propios que se esconden a los 2,8 s; barra con
  lo bufferizado y el tiempo al pasar el mouse (`SeekBar`); menús de subtítulos, audio y velocidad;
  panel de streams a la derecha; pantalla de carga con datos vivos (`PlayerLoading`); diálogo de
  retomar; panel "Try another stream"; "Next episode" en los créditos y cuenta regresiva al final.
- **Cajón (Drawer):** formularios y la lista de streams del detalle, a la derecha sobre un velo.
- **Select**, **Switch**, **Seg**, **Confirm**, **Section**: los de Ethereon, iguales.
- **Avisos (toasts):** entran desde abajo a la derecha; rojos si fallan.
- **Tooltips propios** (`lib/tooltip.ts`): todo `title=` se muestra con demora y estilo del tema.
- **Menú de subtítulos**: primero los del archivo ("Embedded"); después una entrada por idioma con
  su nombre en ese idioma ("Español (Latinoamérica)", "Português (Brasil)"), el preferido y el del
  audio arriba y el resto alfabético; "Other versions (n)" despliega las otras versiones. Abajo,
  "Sync" corre los subtítulos de a 0,5 s (también con G/H, con un aviso del offset).
- **Botones del detalle**: siempre en una fila; en celular "Choose stream" queda solo con el ícono.
- **Cast** (detalle): avatares con iniciales sobre el acento (el catálogo no trae fotos). Debajo,
  la fila "More like this" (mismo género principal).
- **Episodios**: en pantallas anchas van en dos o tres columnas; los especiales (temporada 0) se
  llaman "Specials" y van al final; con más de 6 temporadas, Select en vez de Seg.

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
paneles y menús. En el reproductor: espacio/K, F, M, ←/→ (5 s), J/L (10 s), ↑/↓
volumen, C subtítulos, G/H subtítulos 0,5 s antes/después, N siguiente episodio, Esc (cierra menú → sale de pantalla completa → vuelve).

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
- Sí: imágenes chicas en grillas (`small`), medianas en fondos (`medium`).
- No: colores fijos en componentes. Si hace falta uno, es una variable nueva en `themes.css`.
- No: animaciones largas fuera del hero y la transición del póster.
- No: librerías de UI. Íconos de `@lucide/svelte` importados de a uno.

## 8. Comportamiento responsive

Ventana mínima del desktop: 900 x 560. Las grillas usan `auto-fill`; las filas, anchos con `clamp`.
Hasta 700 px de ancho (celular): la barra lateral pasa a ser una barra abajo con íconos y nombre, el
detalle apila póster y datos, el reproductor esconde la barra de volumen. Márgenes laterales con
`--gutter` (16 a 44 px).

## 9. Guía para agentes

Antes de cambiar la interfaz: leer este archivo, `styles/themes.css` y `docs/API.md`. Probar en los
seis temas (Settings → Appearance) y con "Reduce motion". `npm run check` sin errores ni avisos y
`npm run build`. Sin server: `npm run dev:mock` (datos de Cinemeta guardados y un video de prueba;
usuarios `admin`/`admin` y `test`/`test`). Comentarios en español, textos de la interfaz en inglés.

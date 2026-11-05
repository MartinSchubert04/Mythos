# Pendientes

Lo que falta hacer. Cada tarea se borra de acá apenas se completa (en el mismo commit que la
resuelve); cuando no queda ninguna, se borra el archivo.

## Idioma: lo que quedó después de pasar la interfaz al español

- [ ] Que el server mande un código además del texto en los errores (`{"error": "...", "code":
      "bad_login"}`): hoy la UI reconoce los conocidos por el texto en inglés
      (`apps/web/src/lib/i18n/server.ts`) y si cambia una frase en el backend deja de traducirse.
      Los de `backend/media` (ffmpeg, buffering) y los que llevan detalles (`Sync failed: ...`)
      se muestran a medias o en inglés.

## TV (Samsung / Tizen): segunda pasada con el control remoto — para el 2026-11-05

Lo que quedó de la depuración en la tele (v0.17.0). Se prueba en la UN50DU7000 con la app de
Tizen (docs/tizen.md: DevTools por sdb, control por red, uiproxy en el puerto 7790 con el Python de
`AppData\Local\Programs\Python\Python312`), siempre con el video muteado. Empezar recién cuando
esté publicada la vista previa al adelantar (las dos tocan el reproductor).

- [ ] Recorrer con el control y arreglar lo que aparezca: login con el teclado en pantalla,
      "Who's watching?" y agregar perfil, búsqueda (y atrás con el teclado abierto), Top rated,
      Library, Settings, Admin.
- [ ] Reproductor: panel de episodios, próximo episodio al terminar y la pantalla de server caído.
- [ ] Tráiler de YouTube: no se maneja con el control (atrás lo cierra); ver si reproduce en la
      tele o reemplazarlo en TV.

## Para probar mañana (2026-11-05)

Hecho en código (tests, clippy, `npm run check` y `npm run build` en verde) pero sin probar de
verdad: el 2026-11-04 no se pudo usar la tele ni reproducir. Cuando se pruebe y ande, se borra de
acá (y si algo falla, vuelve arriba como pendiente).

- [ ] **Vista previa al adelantar** (`backend/media/src/thumbs.rs`, `/api/media/{session}/thumbs`,
      `components/SeekBar.svelte`, docs/API.md "Vista previa al adelantar"). Probado solo con el
      ejemplo `play` de media en la PC (Sintel completo: 177/178 en ~40 s; a 300-500 KB/s solo
      aparecen las del tramo bajado). Falta, con el server desplegado:
      - PC (web o desktop): pasar el mouse por la barra → miniatura + tiempo; arrastrar.
      - Android: arrastrar la barra con el dedo.
      - Tele: izquierda/derecha → la barra con foco y la miniatura encima del tiempo (tamaño a 3 m).
      - Con un archivo completo (aparecen todas enseguida) y uno a medio bajar (solo lo bajado; las
        demás van apareciendo; donde no hay, solo el tiempo).
      - Que no trabe la reproducción: mirar CPU del server (ffmpeg de miniaturas con prioridad
        mínima, uno a la vez) y que el video no se corte mientras se generan.
      - Un HDR o 4K HEVC: que salgan (sin tone-mapping se verán lavadas; decidir si importa).
- [ ] **Streams que aprenden el release preferido** (`server/src/taste.rs`, tabla `plays` en la
      migración nueva de `db.rs`, bonus 9. en `score.rs`, docs/API.md "Preferencias aprendidas",
      motivo en `StreamList.svelte`). Cómo probar (en la PC o la tele):
      - Que la migración aplique sobre la base real sin perder nada (hacer copia de `mythos.db`).
      - En una serie (p. ej. Demon Slayer): elegir a mano en "Choose stream" un release de otro grupo
        (Erai-raws), verlo un rato o terminarlo; en el episodio siguiente ese grupo tiene que venir
        primero y "Elegir stream" decir "Mismo grupo que la última vez" (y "Subtítulos en español"
        si los trae; en inglés "Same group as last time" · "Spanish subs").
      - Cambiar de stream a los pocos minutos: ese grupo no se prefiere. Elegir uno desde el cartel
        de "trabado": el anterior queda como trabado.
      - Un preferido con pocos seeders no tiene que ganarle a uno sano.
      - `subLang` de Settings en español: suben los releases que traen subtítulos en español.
- [ ] **Reproductor en TV: foco después de elegir velocidad o audio** (`closeMenu` en
      `Player.svelte`): elegir 1.5× con el control → el foco vuelve al botón de velocidad.
- [ ] **Paneles de streams y episodios en TV**: al abrirlos el foco entra en el actual (no en la X)
      sin que el reproductor se corra de costado.
- [ ] **Sacar algo de "Continue watching" en TV** (`ContinueCard.svelte`, `data-nav-inner` en
      `lib/spatial.svelte.ts`): con el foco en la tarjeta se ve la X; arriba la enfoca, centro la
      saca; abajo vuelve a la tarjeta.
- [ ] **Teclas multimedia** (revisión de código: códigos de `lib/tizen.ts` correctos, registro en
      la app y en la página): probar play/pausa, play, pausa, stop (pausa), adelantar y retroceder
      (mantener: el salto crece) con el control físico, también con un menú del reproductor abierto
      (antes no andaban con un menú abierto).
- [ ] **"Done" del teclado de la tele en el login** (`ImeDone` en `lib/tizen.ts` y
      `lib/spatial.svelte.ts`): usuario → Done pasa a la contraseña → Done entra.
- [ ] Revisado en código sin encontrar problemas (probar igual con el control): pantalla de server
      caído (el foco va a "Retry now"; atrás cierra la app), próximo episodio al terminar (el foco
      va a "Play now"), Select de Settings (atrás lo cierra).
- [ ] **Interfaz en español por defecto, inglés como opción** (`lib/i18n.svelte.ts`, textos en
      `lib/i18n/es` y `lib/i18n/en`). Con `npm run dev:mock` (admin/admin), en los dos idiomas
      (Ajustes → Apariencia → "Idioma / Language"):
      - Que cambiar de idioma repinte todo sin recargar (barra lateral, la vista abierta, toasts, el
        resumen de cada sección de Ajustes) y que sobreviva a recargar y a entrar con otro usuario
        (viaja como `ui.lang`).
      - Textos cortados o que empujen el layout: la barra lateral (212 px y plegada), los botones
        del detalle en una fila (en celular "Elegir stream" solo con el ícono), las tarjetas de
        Seguir viendo / Episodios nuevos, los chips y filtros de Calificadas, las tablas de Admin.
      - Reproductor: menús de subtítulos (Desfase / Tamaño / Posición / Fondo), audio, velocidad,
        el panel de streams, la pantalla de carga, "Probar otro stream", próximo episodio con la
        cuenta regresiva, el `?` de atajos y las estadísticas (D).
      - Escala de TV (modo TV, página de 960 px): login, "¿Quién está viendo?", Ajustes y el
        reproductor, que el español más largo no se salga de los botones ni de los menús.
      - Que no quede nada en inglés en español (y viceversa): buscar textos sueltos en vistas
        poco usadas (Admin → Registros/Tráfico/Avatares, modo local, servidores, dispositivos).
      - Fechas, números y tiempos relativos con el formato del idioma ("hace 5 min", "1,5 GB").
      - El aviso de navegador viejo (`/?webviewTest=66` en localhost) en español y, con
        `localStorage["mythos.lang"] = "en"`, en inglés.
      - App de Tizen: la página local en español con la tele en es_MX.

# Pendientes

Lo que falta hacer. Cada tarea se borra de acá apenas se completa (en el mismo commit que la
resuelve); cuando no queda ninguna, se borra el archivo.

## Streams más inteligentes: aprender qué release prefiere cada usuario

Ejemplo real: en Demon Slayer el usuario elegía a mano "Erai-raws" (traía subtítulos en español).
La app tendría que aprenderlo y ofrecerlo primero en los episodios siguientes, sin perder la
prioridad de una buena conexión.

- [ ] Registrar cada reproducción (usuario, serie/título, episodio): grupo del release (del nombre
      y del `bingeGroup` de Torrentio), resolución, códec, tamaño, audio dual, subtítulos REALES
      del archivo (los que ve ffprobe al abrirlo, no solo lo que dice el nombre), si se eligió a
      mano (Choose stream) o automático, y cómo salió (visto entero, trabas, cambiado a los pocos
      minutos = señal negativa).
- [ ] Inferir el porqué: al elegir a mano, comparar contra el que la app ponía primero (¿la
      elección tenía subtítulos en español y el otro no? ¿audio dual? ¿menos peso?) y guardar esas
      preferencias por usuario (globales) y por serie (el grupo).
- [ ] Usarlo en `score.rs`: bonus al grupo preferido de esa serie y a las características que le
      importan al usuario, acotado para que la salud del torrent siga mandando (un grupo preferido
      casi muerto no le gana a uno sano con las mismas características). Lo de una serie pesa poco
      en las demás; lo global (p. ej. subtítulos en español) pesa en todas.
- [ ] En "Choose stream", el recomendado dice por qué ("Same group as last episode · Spanish subs").
- [ ] Tests del score con casos tipo Demon Slayer (preferido sano gana; preferido muerto pierde
      contra uno sano con subtítulos en español; sin historial, todo como hoy).

## Para probar mañana (2026-11-05): la interfaz en español y en inglés

Solo se verificó con `npm run check` y `npm run build`; falta mirarla. Con `npm run dev:mock`
(admin/admin), en los dos idiomas (Settings → Apariencia → "Idioma / Language"):

- [ ] Que cambiar de idioma repinte todo sin recargar (barra lateral, la vista abierta, toasts, el
      resumen de cada sección de Ajustes) y que sobreviva a recargar y a entrar con otro usuario
      (viaja como `ui.lang`).
- [ ] Textos cortados o que empujen el layout: la barra lateral (212 px y plegada), los botones
      del detalle en una fila (en celular "Elegir stream" solo con el ícono), las tarjetas de
      Seguir viendo / Episodios nuevos, los chips y filtros de Calificadas, las tablas de Admin.
- [ ] Reproductor: menús de subtítulos (Desfase / Tamaño / Posición / Fondo), audio, velocidad,
      el panel de streams, la pantalla de carga, "Probar otro stream", próximo episodio con la
      cuenta regresiva, el `?` de atajos y las estadísticas (D).
- [ ] Escala de TV (modo TV, página de 960 px): login, "¿Quién está viendo?", Ajustes y el
      reproductor, que el español más largo no se salga de los botones ni de los menús.
- [ ] Que no quede nada en inglés en español (y viceversa): buscar textos sueltos en vistas
      poco usadas (Admin → Registros/Tráfico/Avatares, modo local, servidores, dispositivos).
- [ ] Fechas, números y tiempos relativos con el formato del idioma ("hace 5 min", "1,5 GB").
- [ ] El aviso de navegador viejo (`/?webviewTest=66` en localhost) en español y, con
      `localStorage["mythos.lang"] = "en"`, en inglés.
- [ ] App de Tizen: la página local en español con la tele en es_MX.

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

- [ ] Reproductor: después de elegir una velocidad el foco queda perdido (hay un intento de
      arreglo en `Player.svelte` que no funcionó).
- [ ] Recorrer con el control y arreglar lo que aparezca: login con el teclado en pantalla,
      "Who's watching?" y agregar perfil, búsqueda (y atrás con el teclado abierto), Top rated,
      Library, Settings, Admin.
- [ ] Reproductor: panel de episodios, próximo episodio al terminar y la pantalla de server caído.
- [ ] Teclas multimedia (play/pausa/stop/adelantar) con el control físico.
- [ ] Tráiler de YouTube: no se maneja con el control (atrás lo cierra); ver si reproduce en la
      tele o reemplazarlo en TV.
- [ ] Poder sacar algo de "Continue watching" desde la tele.

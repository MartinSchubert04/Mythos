# Pendientes

Lo que falta hacer. Cada tarea se borra de acá apenas se completa (en el mismo commit que la
resuelve); cuando no queda ninguna, se borra el archivo.

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

## Para confirmar en la tele (el resto ya se probó en la PC el 2026-11-05)

Probado en la PC con un server local, Chrome muteado y el modo TV (`?tv=1`, página de 960x540,
teclado en vez del control): anda. Falta confirmarlo con la tele y el control de verdad; si anda,
se borra.

- [ ] **Vista previa al adelantar en la tele**: izquierda/derecha → la barra con foco y la
      miniatura encima del tiempo (tamaño a 3 m). En la PC ya anda (mouse, arrastre, archivo
      completo 177/178 en ~26 s, a medio bajar solo lo bajado, ffmpeg en prioridad Idle de a uno,
      el video sin cortes ni cuadros perdidos, HDR con tone-mapping). Falta también Android
      (arrastrar con el dedo).
- [ ] **Foco con el control** (en la PC con teclado y `?tv=1` anda): después de elegir velocidad
      vuelve al botón (audio no se pudo probar: el mock no tiene varias pistas); los paneles de
      streams y episodios entran en el actual sin correr el reproductor; en "Seguir viendo" arriba
      enfoca la X, centro la saca y abajo vuelve; server caído → "Reintentar ahora"; próximo
      episodio → "Reproducir ya"; el Select de Ajustes se cierra con atrás.
- [ ] **Teclas multimedia** (`lib/tizen.ts`): en la PC, con los nombres de tecla del navegador,
      play/pausa, play, stop, adelantar y retroceder andan también con un menú abierto. Falta con
      el control físico (los códigos de Tizen) y que mantener haga crecer el salto.
- [ ] **"Done" del teclado de la tele en el login** (`ImeDone` en `lib/tizen.ts` y
      `lib/spatial.svelte.ts`): usuario → Done pasa a la contraseña → Done entra.
- [ ] **App de Tizen en español con la tele en es_MX**: la página local con `navigator.language`
      es-MX sale en español en la PC (y en inglés con en-US); falta verla en la tele.

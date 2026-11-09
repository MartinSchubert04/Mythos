# Pendientes

Lo que falta hacer. Cada tarea se borra de acá apenas se completa (en el mismo commit que la
resuelve); cuando no queda ninguna, se borra el archivo.

## Lo que no se pudo probar en la segunda pasada de la tele (2026-11-05)

El resto de la TV Samsung quedó probado con el control real y andando (login, perfiles,
búsqueda, Calificadas, Biblioteca, Ajustes, reproductor, tráiler, teclas multimedia, server
caído, página local en español).

- [ ] **Admin con el control**: la cuenta de la tele (`tv`) no es admin, así que no aparece. Probar
      con una cuenta admin en la tele (o en la PC con `?tv=1`): navegar las pestañas y los
      diálogos de renombrar / cambiar contraseña (el campo abre el teclado; "Done" ahora confirma,
      `components/Confirm.svelte`).
- [ ] **"Seguir viendo": centro sobre la X la saca**. Arriba enfoca la X y abajo vuelve (probado);
      no se apretó para no sacarle una serie al usuario.
- [ ] **Android: vista previa al adelantar arrastrando con el dedo** (en la PC con mouse y en la
      tele con el control ya anda).

## Recomendaciones "para vos" (2026-11-05)

En la tele quedó confirmado el 2026-11-06 con el server en 0.19.0: el destacado sale de lo
recomendado (con descripción y logo), "Películas para vos" y "Series para vos" arriba de
"Populares", "Similares" en la ficha, y el inicio no tarda más que con la 0.18.2 (los pedidos de
"para vos" contestan en ~25 ms; el trabajo pesado de la tele al abrir es el mismo en las dos).

- [ ] **Verlo en la PC**: el inicio y "Similares" con la interfaz de escritorio.

## Tema por usuario (2026-11-06)

Probado en la PC (server de prueba + `npm run dev`, Chrome sin ventana): migracion, cada usuario
con su tema, cambio de cuenta sin recargar, sin parpadeo al abrir y perfiles de la TV con `?tv=1`.

- [ ] **En la tele**: con dos perfiles de temas distintos, cambiar en "¿Quién está viendo?" y ver
      que la pagina nueva arranca ya con el tema del elegido (la primera vez despues de actualizar
      puede verse el anterior un instante: el perfil todavia no tiene la copia de sus preferencias).
- [ ] **Vidrio en desktop**: prenderlo, cambiar la opacidad, salir y entrar con otra cuenta (cada
      una con el suyo); y que un usuario sin `ui.glass` herede el de la PC.

## Desktop: los controles no aparecen al mover el mouse (2026-11-09)

Causa encontrada (reproducida en una instancia de prueba de la app de escritorio, con el server
real y "All Quiet on the Western Front", 1920×802 con franjas negras): con los controles
escondidos unos segundos, WebView2 manda el video a la placa de video como capa aparte y deja de
dibujar la página. Al mover el mouse la clase `hide-ui` se saca y el cursor vuelve, pero los
controles no se ven hasta que cambie otra cosa encima del video. Con videos 16:9 que llenan la
ventana no pasa. No es `DirectCompositionLetterboxVideoOptimization` ni
`DesktopPlaneRemovalForMFFullScreenLetterbox` (apagados no cambió nada).

Arreglo commiteado sin publicar (`views/Player.svelte`: `.desk > video { opacity: 0.999 }`, solo
desktop): inyectado a mano en la página anduvo 6 de 6 ciclos de 8 s escondido (sin él, 1 de 6).

- [ ] **Probarlo con el código real**: el `vite` de 5173 servía el CSS viejo (no tomó el cambio) y
      la prueba con el código se cortó. Levantar `npm run dev`, abrir una película con franjas
      negras en desktop (muteada), esperar >8 s con los controles escondidos y mover el mouse.
      Si anda: versión 0.19.3, tag y push.
- [ ] **¿Pasa también en Chrome/Edge en Windows?** Si pasa, sacar la condición `desk`.

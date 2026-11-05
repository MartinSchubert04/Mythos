# Pendientes

Lo que falta hacer. Cada tarea se borra de acá apenas se completa (en el mismo commit que la
resuelve); cuando no queda ninguna, se borra el archivo.

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

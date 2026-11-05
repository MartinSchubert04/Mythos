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

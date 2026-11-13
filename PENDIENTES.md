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

## Tele: recarga al despausar (2026-11-13)

El desfase de audio en remux (0.22.1) quedo probado en la tele con el inspector: 12 Monkeys 2160p
GalaxyRG (DDP5.1 → AAC), retomando en 40:00 y adelantando a 1:05:00, las dos pistas del primer
segmento arrancan con la misma diferencia que en el archivo original (21 ms de corrimiento, un
cuadro de AAC); la barra marca el segundo correcto.

- [ ] **Recarga al despausar**: no se reprodujo. Con pausas de 3 y 10 minutos siguio al instante,
      sin "Cargando" ni playlist nueva: el reproductor pide la playlist cada 5,5 s y el server no
      cierra ffmpeg. Si vuelve a pasar, fijarse cuanto duro la pausa y si la tele apago la
      pantalla o salio de la app (ahi se cortan esos pedidos); el reproductor ahora espera 12 s
      (antes 4) antes de recargar.

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

## Desktop: controles que no aparecían al mover el mouse (2026-11-09)

Arreglado en 0.19.3 (`views/Player.svelte`, `.desk > video`): con una película con franjas negras
y los controles escondidos unos segundos, WebView2 mandaba el video a la placa como capa aparte y
dejaba de dibujar la página. Probado en una instancia aparte de desktop con el server real: 4 de 4
(sin el arreglo, 1 de 6).

- [ ] **¿Pasa también en Chrome/Edge en Windows?** Probar la web con una película con franjas
      negras, >8 s con los controles escondidos y mover el mouse. Si pasa, sacar la condición
      `desk` (que la regla valga para todos).

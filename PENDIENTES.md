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

## Interfaz en español por defecto — en curso (agente en un worktree)

- [ ] i18n con español por defecto e inglés como opción; se integra cuando termine el agente de
      las miniaturas (los dos tocan el reproductor).

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

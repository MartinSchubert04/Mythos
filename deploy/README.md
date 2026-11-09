# Despliegue de Mythos

Cómo levantar Mythos en el server de casa con Docker. La app queda escuchando solo en
`127.0.0.1:7700` y se publica a la tailnet con `tailscale serve`; desde internet no se ve nada salvo
el puerto de BitTorrent.

## Puertos

| Puerto | Para qué | Dónde |
|---|---|---|
| `127.0.0.1:7700` TCP | API + interfaz web | solo local; afuera por `tailscale serve` (`https://<tu-server>.ts.net:7700`) |
| `42069` TCP + UDP | peers de BitTorrent | abierto en el router (más peers = baja más rápido) |
| UDP aleatorio | DHT (buscar peers sin trackers) | lo elige librqbit; sale solo, no hace falta abrirlo |

### Por qué `network_mode: host`

El contenedor usa la red del host en vez de publicar puertos. El motor de torrents necesita dos
cosas que con la red aislada de Docker no andan: **UPnP** (le pide al router que abra el puerto de
peers; desde la red bridge el router no lo ve) y el **DHT**, que abre un puerto UDP aleatorio que no
se puede publicar de antemano. Sin eso hay menos peers y la película tarda más en arrancar.

La app no queda más expuesta: `MYTHOS_BIND=127.0.0.1:7700` (fijo en `docker-compose.yml`)
hace que escuche solo en localhost, y `tailscale serve` la publica a la tailnet. El healthcheck
(`mythos health`) consulta ese mismo puerto en 127.0.0.1.

**Red de casa (opcional):** con `MYTHOS_EXTRA_BIND=<ip-del-server>:7700` en `deploy/.env` también
se entra desde la LAN por `http://<ip-del-server>:7700`, sin Tailscale (por ejemplo, un TV box). No
sirve `0.0.0.0`: `tailscale serve` ya ocupa el 7700 en la IP de la tailnet. Conviene reservar la
IP del server en el router (DHCP fijo) para que no cambie. Es `http` sin
cifrar y cualquiera conectado al WiFi llega a la pantalla de login (con límite de intentos). Desde
afuera de casa sigue siendo solo por la tailnet, y `tailscale serve` funciona igual.

En el server ya están ocupados 22, 53, 443, 1883, 3000, 4000, 4517, 5678, 6052, 6080, 8080, 8090,
8123, 8443, 8765, 8787, 8788, 10200, 10300, 11987, 51515 y 61208: Mythos no usa ninguno.

## Requisitos

- Docker con el plugin `compose`.
- Driver de NVIDIA y el [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html)
  (para convertir video en la GPU con NVENC). Probá que ande con
  `docker run --rm --gpus all ubuntu nvidia-smi`.
- Tailscale instalado y logueado en el server.

## Con el script (recomendado)

`deploy/deploy.sh` hace todo lo de abajo y se puede correr las veces que haga falta (la primera vez
y en cada actualización):

```bash
mkdir -p ~/Projects && cd ~/Projects
git clone https://github.com/MartinSchubert04/Mythos.git
MYTHOS_ADMIN_USER=martin ./Mythos/deploy/deploy.sh      # el nombre es opcional (por defecto "admin")
```

1. `git pull --ff-only`.
2. Si no existe `deploy/.env`, lo crea desde `.env.example` con una **contraseña de admin aleatoria
   que muestra una sola vez** (anotala). Si no encuentra una GPU NVIDIA pone `MYTHOS_HWACCEL=none`.
   Si el `.env` ya existe no lo toca.
3. Deja `deploy/data/` a nombre del uid 1000 (el usuario del contenedor), con `sudo` si hace falta.
4. `docker compose up -d --build`.
5. Espera hasta 3 minutos a que el healthcheck dé `healthy`; si falla, muestra los últimos logs y
   termina con error.
6. Si `tailscale serve` todavía no publica el puerto 7700, corre
   `tailscale serve --bg --https=7700 http://127.0.0.1:7700` (con `sudo` si hace falta) y muestra la
   URL final.

## A mano (primera vez)

```bash
mkdir -p ~/Projects && cd ~/Projects
git clone https://github.com/MartinSchubert04/Mythos.git
cd Mythos/deploy

cp .env.example .env
nano .env                 # usuario y contraseña del primer admin, MYTHOS_HWACCEL=nvenc

# La carpeta de datos (base de datos + caché de torrents). El contenedor corre con el uid 1000:
mkdir -p data && sudo chown 1000:1000 data

docker compose up -d --build
docker compose logs -f    # tiene que decir "Mythos … escuchando en http://127.0.0.1:7700"
curl http://127.0.0.1:7700/api/health     # {"ok":true,"version":"…"}
```

El primer build tarda varios minutos (compila la UI y el backend). Los siguientes reusan la capa de
dependencias y son mucho más rápidos.

### Publicarlo en la tailnet

```bash
sudo tailscale serve --bg --https=7700 http://127.0.0.1:7700
tailscale serve status
```

Queda en `https://<tu-server>.ts.net:7700`, con HTTPS de verdad y visible solo para los equipos de
tu tailnet. Para sacarlo: `sudo tailscale serve --https=7700 off`.

### Abrir el puerto de BitTorrent en el router

En el router, redirigí el puerto **42069 TCP y UDP** a la IP local del server. No es obligatorio
(sin esto baja igual), pero con el puerto abierto se conectan muchos más peers y la película
arranca y avanza más rápido (UPnP lo intenta solo, pero muchos routers lo traen apagado). Si
cambiás el puerto, cambialo en `MYTHOS_PEER_PORT`. El reenvío tiene que ser al **mismo** puerto
externo e interno: Mythos anuncia `MYTHOS_PEER_PORT` a trackers y DHT. Con doble NAT (módem del
proveedor + router propio) hace falta en los dos, o el módem en modo bridge. Al arrancar, el log
dice `torrent session ready ... announce_port=Some(42069)`; si dice otro puerto (el 42069 estaba
ocupado), avisa con un `warn` y el reenvío no sirve hasta reiniciar con el puerto libre.

Con el puerto abierto, Mythos usa hasta **200 peers por torrent** (entrantes y salientes), sube sin
tope mientras baja (los peers priorizan a quien les sube) y refresca la lista de trackers públicos
cada 12 h. Normalmente hay uno o dos torrents vivos a la vez (lo que no mira nadie se pausa), así
que entra holgado en la tabla NAT de un router de casa (4–16 mil conexiones para toda la casa).
Si al bajar se corta internet en la casa, bajalo con `MYTHOS_PEER_LIMIT=120` en `deploy/.env`.
Detalle: "Peers y NAT" en `backend/media/src/engine.rs`.

## Usuarios

Nadie entra sin cuenta. El **primer admin** se crea al arrancar si la base está vacía, con
`MYTHOS_ADMIN_USER` y `MYTHOS_ADMIN_PASSWORD` del `.env` (después podés borrarlos del archivo). Si
arrancó sin esas variables, el log lo avisa; también se puede crear a mano:

```bash
docker compose exec mythos mythos adduser martin 'una-contraseña-larga' --admin
```

El resto de los usuarios se crean desde la interfaz (sección Admin). Si alguien se olvida la
contraseña (o te quedaste afuera):

```bash
docker compose exec mythos mythos passwd martin 'nueva-contraseña'   # también lo rehabilita
```

`MYTHOS_DEV=1` crea `admin`/`admin` y `test`/`test` para pruebas locales: **nunca** en el server.

## Actualizar

```bash
~/Projects/Mythos/deploy/deploy.sh
# o a mano:
cd ~/Projects/Mythos && git pull
cd deploy && docker compose up -d --build
```

Las migraciones de la base corren solas al arrancar (en el lugar, sin perder datos; conviene
igual un backup antes de actualizar, ver abajo).

## Datos y backups

Todo vive en `deploy/data/`:

- `mythos.db`: usuarios, sesiones, progreso, biblioteca, settings. Es lo único que vale la pena
  respaldar (con `sqlite3` en el host: `sqlite3 data/mythos.db ".backup mythos-backup.db"`, aunque el contenedor esté andando).
- `torrents/`: la caché de descargas. Se limpia sola: lo que no se mira hace 3 días se borra
  (`MYTHOS_CACHE_HOURS`, por defecto 72; 0 = no vence) y, al pasar `MYTHOS_CACHE_GB`, lo usado hace
  más tiempo. Nunca lo que se está viendo. Se puede borrar entera con el contenedor parado.
- `hls/`: temporales de ffmpeg.

### Opcional: buffers UDP más grandes

Si el log dice `couldn't set UDP rcv buf size`, el kernel limita el buffer de los sockets UDP (uTP)
y con mucho tráfico se pierden paquetes. Se puede subir en el host (no se configura por contenedor):

```bash
echo 'net.core.rmem_max=8388608' | sudo tee /etc/sysctl.d/90-mythos.conf
echo 'net.core.wmem_max=8388608' | sudo tee -a /etc/sysctl.d/90-mythos.conf
sudo sysctl --system && docker compose restart
```

## Métricas

El server registra cada request (la ruta como plantilla, nunca la URL con `?t=`) y los eventos de
login y de usuarios, para la sección Admin (`/api/admin/metrics`, `/logs`, `/audit`; ver
`docs/API.md`). Ocupa poco (crudos 14 días, por hora 90 días, en `mythos.db`). Para apagarlo:
`MYTHOS_METRICS=0` en el `.env`.

## Sin GPU

Poné `MYTHOS_HWACCEL=none` en el `.env` y borrá el bloque `reservations: devices:` de
`docker-compose.yml`. ffmpeg convierte por CPU (anda, pero un 4K HEVC en vivo puede no llegar).

## Comprobaciones útiles

```bash
docker compose ps                                        # healthy
docker compose exec mythos ffmpeg -hide_banner -encoders | grep nvenc
docker compose exec mythos ffmpeg -hide_banner -f lavfi -i testsrc=duration=2:size=1280x720 \
  -c:v h264_nvenc -f null -                               # si falla, el runtime de nvidia no está
docker stats mythos                                      # RAM (límite 2 GB en el compose)
```

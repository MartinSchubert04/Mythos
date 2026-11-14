# Deploying Mythos

How to bring up Mythos on the home server with Docker. The app listens only on
`127.0.0.1:7700` and is published to the tailnet with `tailscale serve`; from the internet nothing is visible except
the BitTorrent port.

## Ports

| Port | What for | Where |
|---|---|---|
| `127.0.0.1:7700` TCP | API + web interface | local only; outside via `tailscale serve` (`https://<tu-server>.ts.net:7700`) |
| `42069` TCP + UDP | BitTorrent peers | open on the router (more peers = faster downloads) |
| random UDP | DHT (finding peers without trackers) | chosen by librqbit; outbound only, no need to open it |

### Why `network_mode: host`

The container uses the host's network instead of publishing ports. The torrent engine needs two
things that don't work with Docker's isolated network: **UPnP** (it asks the router to open the peer
port; from the bridge network the router doesn't see it) and the **DHT**, which opens a random UDP port that can't
be published in advance. Without them there are fewer peers and the movie takes longer to start.

The app isn't any more exposed: `MYTHOS_BIND=127.0.0.1:7700` (fixed in `docker-compose.yml`)
makes it listen only on localhost, and `tailscale serve` publishes it to the tailnet. The healthcheck
(`mythos health`) queries that same port on 127.0.0.1.

**Home network (optional):** with `MYTHOS_EXTRA_BIND=<ip-del-server>:7700` in `deploy/.env` it can
also be reached from the LAN at `http://<ip-del-server>:7700`, without Tailscale (for example, a TV box).
`0.0.0.0` doesn't work: `tailscale serve` already takes 7700 on the tailnet IP. It's a good idea to reserve the
server's IP on the router (static DHCP) so it doesn't change. It's unencrypted `http` and
anyone connected to the WiFi reaches the login screen (with an attempt limit). From
outside the house it's still only through the tailnet, and `tailscale serve` works the same.

On the server 22, 53, 443, 1883, 3000, 4000, 4517, 5678, 6052, 6080, 8080, 8090,
8123, 8443, 8765, 8787, 8788, 10200, 10300, 11987, 51515 and 61208 are already taken: Mythos uses none of them.

## Requirements

- Docker with the `compose` plugin.
- NVIDIA driver and the [NVIDIA Container Toolkit](https://docs.nvidia.com/datacenter/cloud-native/container-toolkit/latest/install-guide.html)
  (to convert video on the GPU with NVENC). Check that it works with
  `docker run --rm --gpus all ubuntu nvidia-smi`.
- Tailscale installed and logged in on the server.

## With the script (recommended)

`deploy/deploy.sh` does everything below and can be run as many times as needed (the first time
and on every update):

```bash
mkdir -p ~/Projects && cd ~/Projects
git clone https://github.com/MartinSchubert04/Mythos.git
MYTHOS_ADMIN_USER=martin ./Mythos/deploy/deploy.sh      # the name is optional (default "admin")
```

1. `git pull --ff-only`.
2. If `deploy/.env` doesn't exist, it creates it from `.env.example` with a **random admin password
   that it shows only once** (write it down). If it doesn't find an NVIDIA GPU it sets `MYTHOS_HWACCEL=none`.
   If the `.env` already exists it doesn't touch it.
3. Gives `deploy/data/` to uid 1000 (the container's user), with `sudo` if needed.
4. `docker compose up -d --build`.
5. Waits up to 3 minutes for the healthcheck to report `healthy`; if it fails, it shows the latest logs and
   exits with an error.
6. If `tailscale serve` isn't publishing port 7700 yet, it runs
   `tailscale serve --bg --https=7700 http://127.0.0.1:7700` (with `sudo` if needed) and shows the
   final URL.

## By hand (first time)

```bash
mkdir -p ~/Projects && cd ~/Projects
git clone https://github.com/MartinSchubert04/Mythos.git
cd Mythos/deploy

cp .env.example .env
nano .env                 # user and password of the first admin, MYTHOS_HWACCEL=nvenc

# The data folder (database + torrent cache). The container runs as uid 1000:
mkdir -p data && sudo chown 1000:1000 data

docker compose up -d --build
docker compose logs -f    # it should say "Mythos … escuchando en http://127.0.0.1:7700"
curl http://127.0.0.1:7700/api/health     # {"ok":true,"version":"…"}
```

The first build takes several minutes (it compiles the UI and the backend). The following ones reuse the
dependency layer and are much faster.

### Publishing it on the tailnet

```bash
sudo tailscale serve --bg --https=7700 http://127.0.0.1:7700
tailscale serve status
```

It ends up at `https://<tu-server>.ts.net:7700`, with real HTTPS and visible only to the devices on
your tailnet. To remove it: `sudo tailscale serve --https=7700 off`.

### Opening the BitTorrent port on the router

On the router, forward port **42069 TCP and UDP** to the server's local IP. It's not required
(it downloads anyway without it), but with the port open many more peers connect and the movie
starts and progresses faster (UPnP tries it on its own, but many routers ship with it off). If
you change the port, change it in `MYTHOS_PEER_PORT`. The forwarding has to be to the **same** external
and internal port: Mythos announces `MYTHOS_PEER_PORT` to trackers and DHT. With double NAT (ISP
modem + your own router) it's needed on both, or the modem in bridge mode. On startup, the log
says `torrent session ready ... announce_port=Some(42069)`; if it says another port (42069 was
taken), it warns with a `warn` and the forwarding is useless until restarting with the port free.

With the port open, Mythos uses up to **200 peers per torrent** (inbound and outbound), uploads without
a cap while downloading (peers prioritize whoever uploads to them) and refreshes the public tracker list
every 12 h. Normally there are one or two live torrents at a time (whatever nobody is watching is paused), so
it fits comfortably in a home router's NAT table (4–16 thousand connections for the whole house).
If the house's internet drops while downloading, lower it with `MYTHOS_PEER_LIMIT=120` in `deploy/.env`.
Details: "Peers y NAT" in `backend/media/src/engine.rs`.

## Users

Nobody gets in without an account. The **first admin** is created on startup if the database is empty, with
`MYTHOS_ADMIN_USER` and `MYTHOS_ADMIN_PASSWORD` from the `.env` (you can delete them from the file afterwards). If
it started without those variables, the log says so; it can also be created by hand:

```bash
docker compose exec mythos mythos adduser martin 'una-contraseña-larga' --admin
```

The rest of the users are created from the interface (Admin section). If someone forgets their
password (or you got locked out):

```bash
docker compose exec mythos mythos passwd martin 'nueva-contraseña'   # also re-enables the user
```

`MYTHOS_DEV=1` creates `admin`/`admin` and `test`/`test` for local testing: **never** on the server.

## Updating

```bash
~/Projects/Mythos/deploy/deploy.sh
# or by hand:
cd ~/Projects/Mythos && git pull
cd deploy && docker compose up -d --build
```

Database migrations run on their own on startup (in place, without losing data; still, a backup
before updating is a good idea, see below).

## Data and backups

Everything lives in `deploy/data/`:

- `mythos.db`: users, sessions, progress, library, settings. It's the only thing worth
  backing up (with `sqlite3` on the host: `sqlite3 data/mythos.db ".backup mythos-backup.db"`, even while the container is running).
- `torrents/`: the download cache. It cleans itself up: whatever hasn't been watched in 3 days is deleted
  (`MYTHOS_CACHE_HOURS`, default 72; 0 = never expires) and, when going over `MYTHOS_CACHE_GB`, whatever was used
  longest ago. Never what's being watched. It can be deleted entirely with the container stopped.
- `hls/`: ffmpeg temporary files.

### Optional: bigger UDP buffers

If the log says `couldn't set UDP rcv buf size`, the kernel limits the buffer of UDP (uTP) sockets
and under heavy traffic packets get lost. It can be raised on the host (it's not configured per container):

```bash
echo 'net.core.rmem_max=8388608' | sudo tee /etc/sysctl.d/90-mythos.conf
echo 'net.core.wmem_max=8388608' | sudo tee -a /etc/sysctl.d/90-mythos.conf
sudo sysctl --system && docker compose restart
```

## Metrics

The server logs every request (the route as a template, never the URL with `?t=`) and the login and
user events, for the Admin section (`/api/admin/metrics`, `/logs`, `/audit`; see
`docs/API.md`). It takes little space (raw 14 days, hourly 90 days, in `mythos.db`). To turn it off:
`MYTHOS_METRICS=0` in the `.env`.

## No GPU

Set `MYTHOS_HWACCEL=none` in the `.env` and delete the `reservations: devices:` block from
`docker-compose.yml`. ffmpeg converts on the CPU (it works, but a live 4K HEVC may not keep up).

## Useful checks

```bash
docker compose ps                                        # healthy
docker compose exec mythos ffmpeg -hide_banner -encoders | grep nvenc
docker compose exec mythos ffmpeg -hide_banner -f lavfi -i testsrc=duration=2:size=1280x720 \
  -c:v h264_nvenc -f null -                               # if it fails, the nvidia runtime isn't there
docker stats mythos                                      # RAM (2 GB limit in the compose)
```

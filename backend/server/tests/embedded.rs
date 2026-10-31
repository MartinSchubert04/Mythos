//! Pruebas de punta a punta con la biblioteca embebida: un server "remoto" (modo server) y uno
//! "local" (modo local, como en las apps), los dos en 127.0.0.1:0, hablándose por HTTP de verdad.
//! Sin ffmpeg ni trackers extra (más rápido y sin depender de la red).
use std::{path::PathBuf, time::Duration};

use mythos::{Config, Handle, Mode};
use serde_json::{json, Value};

fn tmp(name: &str) -> PathBuf {
    let nanos = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
    std::env::temp_dir().join(format!("mythos-test-{name}-{nanos}"))
}

fn quiet(mut c: Config) -> Config {
    c.extra_trackers = false;
    c.ffmpeg = PathBuf::new();
    c.ffprobe = PathBuf::new();
    c.cache_gb = 1.0;
    c
}

async fn remote_server(dir: &PathBuf) -> Handle {
    let mut c = quiet(Config::server(dir));
    c.bind = "127.0.0.1:0".parse().unwrap();
    c.web_dir = None;
    c.peer_port = 0;
    c.admin_user = Some("ana".into());
    c.admin_password = Some("secret123".into());
    mythos::run(c).await.expect("remote")
}

async fn local_server(dir: &PathBuf, remote: Option<String>) -> Handle {
    let mut c = quiet(Config::local(dir));
    c.remote_url = remote;
    c.sync_interval = None; // a pedido, para que la prueba sea determinista
    mythos::run(c).await.expect("local")
}

struct Api {
    http: reqwest::Client,
    base: String,
}

impl Api {
    fn new(h: &Handle) -> Self {
        Self { http: reqwest::Client::new(), base: h.base_url() }
    }

    async fn call(&self, method: &str, path: &str, token: Option<&str>, body: Option<Value>) -> (u16, Value) {
        let mut r = self.http.request(method.parse().unwrap(), format!("{}{path}", self.base));
        if let Some(t) = token {
            r = r.bearer_auth(t);
        }
        if let Some(b) = body {
            r = r.json(&b);
        }
        let res = r.send().await.unwrap();
        let st = res.status().as_u16();
        let text = res.text().await.unwrap();
        (st, serde_json::from_str(&text).unwrap_or(Value::Null))
    }
}

fn movie(id: &str) -> Value {
    json!({ "id": id, "type": "movie", "name": format!("Movie {id}") })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn local_mode_end_to_end() {
    let (rdir, ldir) = (tmp("remote"), tmp("local"));
    let remote = remote_server(&rdir).await;
    assert_eq!(remote.mode(), Mode::Server);
    assert_ne!(remote.port(), 0);
    let r = Api::new(&remote);
    let (st, h) = r.call("GET", "/api/health", None, None).await;
    assert_eq!(st, 200);
    assert_eq!(h["mode"], "server");
    assert_eq!(h["transcode"], false);

    let local = local_server(&ldir, Some(remote.base_url())).await;
    let l = Api::new(&local);
    assert_eq!(l.call("GET", "/api/health", None, None).await.1["mode"], "local");

    // --- login online a través del local: enrola la cuenta y devuelve las dos sesiones ---
    let (st, v) = l.call("POST", "/api/auth/login", None, Some(json!({"username": "ana", "password": "nope"}))).await;
    assert_eq!(st, 401, "{v}");
    let (st, v) = l.call("POST", "/api/auth/login", None, Some(json!({"username": "ANA", "password": "secret123", "device": "Test phone"}))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["offline"], false);
    assert_eq!(v["user"]["username"], "ana");
    assert_eq!(v["user"]["is_admin"], false, "en local nadie es admin");
    let ltok = v["token"].as_str().unwrap().to_string();
    let rtok = v["remote"]["token"].as_str().unwrap().to_string();
    assert_eq!(v["remote"]["url"], remote.base_url());
    assert!(!v.to_string().contains("argon2"), "nunca un hash");
    // El token remoto sirve contra el remoto.
    assert_eq!(r.call("GET", "/api/me", Some(&rtok), None).await.1["username"], "ana");
    // El sync automático tras el login corre en segundo plano: esperar a que termine.
    tokio::time::sleep(Duration::from_millis(500)).await;

    // --- cambios offline-first en el local, sync, y aparecen en el remoto ---
    let prog = json!({"meta_id": "tt1375666", "type": "movie", "name": "Inception", "position_s": 600.0, "duration_s": 8880.0});
    assert_eq!(l.call("PUT", "/api/progress", Some(&ltok), Some(prog)).await.0, 204);
    assert_eq!(l.call("PUT", "/api/library/movie/tt0111161", Some(&ltok), Some(movie("tt0111161"))).await.0, 204);
    assert_eq!(l.call("PUT", "/api/me/settings", Some(&ltok), Some(json!({"ui": {"theme": "dark"}, "rd_key": "LOCALRDKEY123"}))).await.0, 200);
    let (_, s) = l.call("GET", "/api/local/status", Some(&ltok), None).await;
    assert_eq!(s["mode"], "local");
    assert_eq!(s["remote_reachable"], true);
    assert_eq!(s["remote_signed_in"], true);
    assert_eq!(s["pending_changes"], 3, "{s}");
    let (st, rep) = l.call("POST", "/api/local/sync", Some(&ltok), None).await;
    assert_eq!(st, 200, "{rep}");
    assert_eq!(rep["pushed"], 3);
    assert_eq!(l.call("GET", "/api/local/status", Some(&ltok), None).await.1["pending_changes"], 0);
    let (st, p) = r.call("GET", "/api/progress/tt1375666", Some(&rtok), None).await;
    assert_eq!((st, p["position_s"].as_f64()), (200, Some(600.0)));
    let (_, rs) = r.call("GET", "/api/me/settings", Some(&rtok), None).await;
    assert_eq!(rs["ui"]["theme"], "dark");
    assert_eq!(rs["has_rd_key"], false, "la clave de RD no viaja");

    // --- cambios en el remoto (otro dispositivo) bajan al local; borrados con lápida ---
    assert_eq!(r.call("PUT", "/api/library/movie/tt0068646", Some(&rtok), Some(movie("tt0068646"))).await.0, 204);
    assert_eq!(r.call("DELETE", "/api/library/movie/tt0111161", Some(&rtok), None).await.0, 204);
    let (st, rep) = l.call("POST", "/api/local/sync", Some(&ltok), None).await;
    assert_eq!(st, 200, "{rep}");
    assert_eq!(rep["pulled"], 2, "{rep}");
    let (_, lib) = l.call("GET", "/api/library", Some(&ltok), None).await;
    let ids: Vec<&str> = lib.as_array().unwrap().iter().map(|m| m["id"].as_str().unwrap()).collect();
    assert_eq!(ids, vec!["tt0068646"]);
    // Sync de nuevo: nada nuevo, idempotente y sin rebotes.
    let (_, rep) = l.call("POST", "/api/local/sync", Some(&ltok), None).await;
    assert_eq!((rep["pushed"].as_i64(), rep["pulled"].as_i64()), (Some(0), Some(0)), "{rep}");

    // --- modo local: sin admin, sin cambio de contraseña, solo Host de loopback ---
    assert_eq!(l.call("GET", "/api/admin/users", Some(&ltok), None).await.0, 403);
    assert_eq!(l.call("GET", "/api/admin/metrics", Some(&ltok), None).await.0, 403);
    assert_eq!(l.call("PUT", "/api/me/password", Some(&ltok), Some(json!({"current": "secret123", "new": "x1234567"}))).await.0, 403);
    let res = l.http.get(format!("{}/api/health", l.base)).header("host", "evil.example:80").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 403);
    let res = l.http.get(format!("{}/api/health", l.base)).header("host", "localhost").send().await.unwrap();
    assert_eq!(res.status().as_u16(), 200);
    // La UI (Tauri) puede preguntar por CORS con Private Network Access.
    let res = l
        .http
        .request(reqwest::Method::OPTIONS, format!("{}/api/me", l.base))
        .header("origin", "http://tauri.localhost")
        .header("access-control-request-method", "GET")
        .header("access-control-request-private-network", "true")
        .send()
        .await
        .unwrap();
    assert_eq!(res.headers().get("access-control-allow-private-network").and_then(|v| v.to_str().ok()), Some("true"));

    // --- métricas del remoto: redacción de `?t=` y 403 para no admins ---
    let secret = "SuperSecretTokenValue123";
    let _ = r.call("GET", &format!("/api/media/abcdef/file?t={secret}"), None, None).await;
    let (st, v) = r.call("POST", "/api/auth/login", None, Some(json!({"username": "ana", "password": "secret123", "device": "admin-cli"}))).await;
    assert_eq!(st, 200);
    let admin = v["token"].as_str().unwrap().to_string();
    let (st, _) = r.call("POST", "/api/admin/users", Some(&admin), Some(json!({"username": "bob", "password": "hunter22"}))).await;
    assert_eq!(st, 200);
    let (_, v) = r.call("POST", "/api/auth/login", None, Some(json!({"username": "bob", "password": "hunter22"}))).await;
    let bob = v["token"].as_str().unwrap().to_string();
    for p in ["/api/admin/metrics", "/api/admin/logs", "/api/admin/audit"] {
        assert_eq!(r.call("GET", p, Some(&bob), None).await.0, 403, "{p}");
    }
    // El escritor junta eventos hasta 2 s.
    let mut logs = Value::Null;
    for _ in 0..40 {
        tokio::time::sleep(Duration::from_millis(250)).await;
        logs = r.call("GET", "/api/admin/logs?route=/api/media/&limit=50", Some(&admin), None).await.1;
        if logs["items"].as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
    }
    let items = logs["items"].as_array().expect("logs");
    assert!(items.iter().any(|i| i["route"] == "/api/media/{session}/file" && i["status"] == 401), "{logs}");
    let (st, m) = r.call("GET", "/api/admin/metrics?range=24h", Some(&admin), None).await;
    assert_eq!(st, 200, "{m}");
    assert!(m["totals"]["requests"].as_i64().unwrap() > 5);
    assert!(m["top_routes"].as_array().unwrap().iter().any(|x| x["route"] == "/api/sync"));
    let mut audit = Value::Null;
    for _ in 0..40 {
        audit = r.call("GET", "/api/admin/audit?event=user_created", Some(&admin), None).await.1;
        if audit["items"].as_array().is_some_and(|a| !a.is_empty()) {
            break;
        }
        tokio::time::sleep(Duration::from_millis(250)).await;
    }
    assert_eq!(audit["items"][0]["detail"].as_str().map(|d| d.contains("bob")), Some(true), "{audit}");
    assert_eq!(r.call("GET", "/api/admin/metrics?range=1y", Some(&admin), None).await.0, 400);

    // --- el remoto se cae: login offline con la verificación local ---
    remote.shutdown().await.unwrap();
    let (st, v) = l.call("POST", "/api/auth/login", None, Some(json!({"username": "ana", "password": "secret123"}))).await;
    assert_eq!(st, 200, "{v}");
    assert_eq!(v["offline"], true);
    assert!(v["remote"].is_null());
    let ltok2 = v["token"].as_str().unwrap().to_string();
    let (st, _) = l.call("POST", "/api/auth/login", None, Some(json!({"username": "ana", "password": "wrong-pass"}))).await;
    assert_eq!(st, 401);
    // Offline sigue andando todo lo de usuario.
    assert_eq!(l.call("PUT", "/api/library/movie/tt0050083", Some(&ltok2), Some(movie("tt0050083"))).await.0, 204);
    let (_, s) = l.call("GET", "/api/local/status", Some(&ltok2), None).await;
    assert_eq!(s["remote_reachable"], false);
    assert_eq!(s["pending_changes"], 1);
    let (st, _) = l.call("POST", "/api/local/sync", Some(&ltok2), None).await;
    assert_eq!(st, 502, "sin server la sincronización falla con un error claro");

    // --- apagado del local: el puerto queda libre ---
    let addr = local.addr();
    local.shutdown().await.unwrap();
    std::net::TcpListener::bind(addr).expect("el puerto quedó libre");

    // --- las métricas en la base nunca tienen el token ---
    {
        let c = rusqlite::Connection::open(rdir.join("mythos.db")).unwrap();
        let mut stmt = c.prepare("SELECT COALESCE(ip,'') || COALESCE(ts_login,'') || method || route FROM request_log").unwrap();
        let rows: Vec<String> = stmt.query_map([], |r| r.get(0)).unwrap().map(Result::unwrap).collect();
        assert!(!rows.is_empty());
        assert!(rows.iter().all(|r| !r.contains(secret) && !r.contains("?t=")), "{rows:?}");
        let audit: String = c
            .query_row("SELECT GROUP_CONCAT(COALESCE(username,'') || COALESCE(detail,'')) FROM audit", [], |r| r.get(0))
            .unwrap();
        assert!(!audit.contains("secret123") && !audit.contains("hunter22"), "{audit}");
    }
    let _ = std::fs::remove_dir_all(&rdir);
    let _ = std::fs::remove_dir_all(&ldir);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn embedded_run_and_shutdown_on_port_zero() {
    let dir = tmp("solo");
    // Local sin remoto configurado: arranca igual, y no hay cómo loguearse sin haberlo hecho
    // online alguna vez.
    let h = local_server(&dir, None).await;
    assert!(h.base_url().starts_with("http://127.0.0.1:"));
    assert!(!h.media_tools().ffmpeg);
    let api = Api::new(&h);
    let (st, v) = api.call("POST", "/api/auth/login", None, Some(json!({"username": "ana", "password": "secret123"}))).await;
    assert_eq!(st, 401);
    assert!(v["error"].as_str().unwrap().contains("offline"), "{v}");
    assert_eq!(api.call("GET", "/api/library", None, None).await.0, 401, "nada sin login");
    let addr = h.addr();
    h.shutdown().await.unwrap();
    assert!(reqwest::get(format!("http://{addr}/api/health")).await.is_err(), "ya no escucha");
    // Se puede volver a arrancar sobre la misma carpeta (la base migrada se reabre).
    let h2 = local_server(&dir, None).await;
    h2.shutdown().await.unwrap();
    let _ = std::fs::remove_dir_all(&dir);
}

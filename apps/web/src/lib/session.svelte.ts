// Sesion: quien entro, sus ajustes del server, y el paso al login cuando el token no sirve.
import { api, ApiError, dropCache, getLocalToken, getToken, onUnauthorized, remoteToken, setToken } from "./api";
import { conn, loginLocal, logoutAll, onLocalExpired, startConn, viaLocal } from "./local.svelte";
import { remoteBase, serverBase } from "./platform";
import { applyRemoteUi } from "./prefs.svelte";
import { go } from "./router.svelte";
import { takeHandoff, toServer } from "./shell.svelte";
import type { Settings, User } from "./types";

export const session = $state({
  /** Ya se sabe si hay sesion (mientras tanto se ve el splash, nunca una pantalla en blanco). */
  checked: false,
  user: null as User | null,
  settings: null as Settings | null,
  /** Habia sesion y el server la rechazo: el login lo dice ("Session expired"). */
  expired: false,
  /** Al abrir no se pudo llegar al server (con un token guardado): pantalla de reconexion. */
  offline: false,
});

function expired() {
  if (session.user) session.expired = true;
  session.user = null;
  session.settings = null;
  session.checked = true;
  conn.enrolled = !!getLocalToken();
}
onUnauthorized(expired);
onLocalExpired(expired);

async function loadSettings() {
  try {
    const s = await api.settings();
    session.settings = s;
    if (s?.ui) applyRemoteUi(s.ui);
  } catch {}
}

export async function start() {
  // Android, pagina del APK con "interfaz del server" prendida: si el server contesta, a su
  // interfaz (mientras, el splash).
  if (await toServer(serverBase())) return;
  // Android, interfaz del server: el token si recien se entro desde la pagina del APK.
  const handed = await takeHandoff();
  if (handed) setToken(handed);
  // Desktop y APK: Auto / Solo local (lib/local.svelte.ts) eligen con que server arrancar.
  await startConn();
  if (!getToken()) {
    session.checked = true;
    return;
  }
  try {
    const [me] = await Promise.all([api.me(), loadSettings()]);
    session.user = me;
    session.offline = false;
    // "Change server" desde la interfaz del server, ya con sesion aca: a Settings (el campo del
    // server y la opcion de la interfaz del server estan ahi). Sin sesion, el login lo muestra.
    if (location.hash.startsWith("#/server")) go("/settings", { replace: true });
  } catch (e) {
    // Sin conexion: no se pierde la sesion, se reintenta (App muestra la pantalla de reconexion).
    // Token vencido: al login, con el aviso.
    // (un proxy delante del server contesta 502/503/504 mientras el server reinicia: tambien es
    // "sin conexion", no "sin sesion").
    if (e instanceof TypeError || (e instanceof ApiError && [502, 503, 504].includes(e.status))) session.offline = true;
    else if (e instanceof ApiError && e.status === 401) session.expired = true;
  } finally {
    session.checked = true;
  }
}

export async function login(username: string, password: string) {
  let user: User;
  if (await viaLocal()) {
    // Por el server local: lo valida contra el de casa si lo alcanza y guarda las dos sesiones.
    ({ user } = await loginLocal(username, password));
    const remote = remoteToken();
    if (conn.on === "server" && remote && (await toServer(remoteBase(), remote, true))) return;
  } else {
    const r = await api.login(username, password);
    setToken(r.token);
    user = r.user;
    // Android, pagina del APK: se sigue en la interfaz del server, ya con la sesion.
    if (await toServer(serverBase(), r.token, true)) return;
  }
  await loadSettings();
  session.expired = false;
  session.user = user;
}

/** Cierra la sesion (en desktop y Android, la del server de casa y la del dispositivo). Tambien
 *  sirve para cambiar de cuenta: queda el login, con el mismo server. */
export async function logout() {
  if (conn.available) await logoutAll();
  else {
    await api.logout().catch(() => {});
    setToken(null);
  }
  dropCache("");
  session.user = null;
  session.settings = null;
  session.expired = false;
}

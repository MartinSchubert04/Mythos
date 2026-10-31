// Sesion: quien entro, sus ajustes del server, y el paso al login cuando el token no sirve.
import { api, ApiError, dropCache, getLocalToken, getToken, onUnauthorized, remoteToken, setToken } from "./api";
import { conn, loginLocal, logoutAll, onLocalExpired, startConn, viaLocal } from "./local.svelte";
import { remoteBase, serverBase } from "./platform";
import { choose, multi } from "./servers.svelte";
import { applyRemoteUi } from "./prefs.svelte";
import { currentProfile, dropCurrentProfile, profiles, rememberProfile, removeProfile, shouldPick, syncProfile } from "./profiles.svelte";
import { go } from "./router.svelte";
import { takeHandoff, toServer } from "./shell.svelte";
import { tv } from "./tv.svelte";
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
  // TV: ese perfil ya no sirve (lo revocaron); si quedan otros, a elegir.
  if (tv.on) {
    dropCurrentProfile();
    if (profiles.list.length) profiles.picking = true;
  }
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
  // Varias direcciones del server de casa (lib/servers.svelte.ts): con cual arrancar (la
  // preferida si contesta, si no la mas rapida). En Solo local no hace falta esperar.
  if (multi()) {
    const pick = choose("start");
    if (conn.mode !== "local") await pick;
  }
  // Android, pagina del APK con "interfaz del server" prendida: si el server contesta, a su
  // interfaz (mientras, el splash).
  if (await toServer(serverBase())) return;
  // Android, interfaz del server: el token si recien se entro desde la pagina del APK.
  const handed = await takeHandoff();
  if (handed) setToken(handed);
  // Desktop y APK: Auto / Solo local (lib/local.svelte.ts) eligen con que server arrancar.
  await startConn();
  // TV con varios perfiles: "Who's watching?" (mientras tanto ya carga el ultimo usado).
  if (shouldPick()) profiles.picking = true;
  if (!getToken()) {
    if (tv.on && profiles.list.length) profiles.picking = true;
    session.checked = true;
    return;
  }
  try {
    const [me] = await Promise.all([api.me(), loadSettings()]);
    session.user = me;
    syncProfile(me);
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
    // En la TV la sesion no vence y la cuenta queda como perfil (lib/profiles.svelte.ts).
    const r = await api.login(username, password, tv.on);
    setToken(r.token);
    user = r.user;
    rememberProfile(r.user, r.token);
    // Se sumo un perfil con otro ya abierto: de cero, sin nada de la cuenta anterior.
    if (profiles.adding && session.user) {
      location.hash = "#/";
      location.reload();
      return;
    }
    profiles.adding = false;
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
  // TV: salir es quitar este perfil del aparato; si quedan otros, a elegir.
  const p = tv.on ? currentProfile() : undefined;
  if (p) {
    await removeProfile(p);
    dropCache("");
    session.user = null;
    session.settings = null;
    session.expired = false;
    if (profiles.list.length) profiles.picking = true;
    return;
  }
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

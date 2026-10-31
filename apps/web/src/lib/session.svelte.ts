// Sesion: quien entro, sus ajustes del server, y el paso al login cuando el token no sirve.
import { api, ApiError, getToken, onUnauthorized, setToken } from "./api";
import { applyRemoteUi } from "./prefs.svelte";
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

onUnauthorized(() => {
  if (session.user) session.expired = true;
  session.user = null;
  session.settings = null;
  session.checked = true;
});

async function loadSettings() {
  try {
    const s = await api.settings();
    session.settings = s;
    if (s?.ui) applyRemoteUi(s.ui);
  } catch {}
}

export async function start() {
  if (!getToken()) {
    session.checked = true;
    return;
  }
  try {
    const [me] = await Promise.all([api.me(), loadSettings()]);
    session.user = me;
    session.offline = false;
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
  const r = await api.login(username, password);
  setToken(r.token);
  await loadSettings();
  session.expired = false;
  session.user = r.user;
}

export async function logout() {
  await api.logout().catch(() => {});
  setToken(null);
  session.user = null;
  session.settings = null;
}

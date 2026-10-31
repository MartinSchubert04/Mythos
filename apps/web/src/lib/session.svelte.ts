// Sesion: quien entro, sus ajustes del server, y el paso al login cuando el token no sirve.
import { api, getToken, onUnauthorized, setToken } from "./api";
import { applyRemoteUi } from "./prefs.svelte";
import type { Settings, User } from "./types";

export const session = $state({
  /** Ya se sabe si hay sesion (mientras tanto se ve el splash, nunca una pantalla en blanco). */
  checked: false,
  user: null as User | null,
  settings: null as Settings | null,
});

onUnauthorized(() => {
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
  } catch {
    // Sin server o token vencido: al login (que muestra el error si no hay conexion).
  } finally {
    session.checked = true;
  }
}

export async function login(username: string, password: string) {
  const r = await api.login(username, password);
  setToken(r.token);
  await loadSettings();
  session.user = r.user;
}

export async function logout() {
  await api.logout().catch(() => {});
  setToken(null);
  session.user = null;
  session.settings = null;
}

// Perfiles en la TV (docs/mobile.md, "TV"): varias cuentas guardadas en el aparato, como en
// Netflix. Cada una entra una sola vez con su contraseña y una sesion que no vence (`persistent`,
// docs/API.md); despues se cambia de una a otra sin pedir nada. Al abrir, con mas de un perfil,
// "Who's watching?" (views/Profiles.svelte) con el ultimo usado elegido.
//
// - Se guardan en el almacenamiento de esta pagina (localStorage): server, usuario, token y
//   avatar. El token es el mismo que ya se guardaba para la sesion activa (`mythos.token`).
// - Cambiar de perfil pone su token (y su server) y recarga la pagina: ninguna cache ni vista
//   queda con datos de la cuenta anterior.
// - Quitar un perfil cierra su sesion en el server. Si el server la revoco (Settings → Signed-in
//   devices de otro aparato, o el admin), el perfil se borra solo al recibir el 401.
// - Solo en TV. En el celular, la PC y la web "Switch account" sigue siendo salir y entrar (con el
//   modo local hay dos sesiones por cuenta y no se guardan perfiles).
import { api, getToken, setToken } from "./api";
import { pickServer, remoteBase } from "./platform";
import { isOurs, useServer } from "./servers.svelte";
import { readJson, write } from "./store";
import { tv } from "./tv.svelte";
import type { User } from "./types";

export type Profile = {
  /** El server de casa de esa cuenta ("" = el mismo origen): la direccion con la que se entro
   *  (si el server tiene varias, cualquiera de ellas es el mismo server). */
  server: string;
  userId: number;
  username: string;
  token: string;
  avatarId: number | null;
};

const KEY = "mythos.profiles";
/** En esta sesion de la pagina ya se eligio (la recarga al cambiar no vuelve a preguntar). */
const PICKED = "mythos.profilePicked";

export const profiles = $state({
  list: readJson<{ list: Profile[] }>(KEY, { list: [] }).list,
  /** Se ve "Who's watching?". */
  picking: false,
  /** Se esta entrando con otra cuenta para sumarla (el login con "Back to profiles"). */
  adding: false,
});

/** El token de la sesion activa al abrir o al entrar (api.ts lo borra antes de avisar un 401). */
let activeToken = getToken();

function persist() {
  write(KEY, JSON.stringify({ list: profiles.list }));
}

/** El mismo server (por cualquiera de sus direcciones) y la misma cuenta. */
const sameServer = (a: string, b: string) => a === b || (isOurs(a) && isOurs(b));
const same = (p: Profile, server: string, userId: number) => sameServer(p.server, server) && p.userId === userId;
/** Donde hablarle al server de un perfil (avatares, logout): la direccion en uso si es el de casa. */
export const profileBase = (p: Profile) => (isOurs(p.server) ? remoteBase() : p.server);

/** El perfil de la sesion activa. */
export function currentProfile(): Profile | undefined {
  const t = getToken();
  return t ? profiles.list.find((p) => p.token === t) : undefined;
}

/** Despues de un login en la TV: lo guarda (o actualiza el token de ese usuario). */
export function rememberProfile(user: User, token: string) {
  if (!tv.on) return;
  const server = remoteBase();
  const p: Profile = { server, userId: user.id, username: user.username, token, avatarId: user.avatar_id ?? null };
  const i = profiles.list.findIndex((x) => same(x, server, user.id));
  if (i >= 0) profiles.list[i] = p;
  else profiles.list.push(p);
  activeToken = token;
  persist();
  markPicked();
}

/** El usuario de la sesion activa cambio (avatar): el perfil tambien. */
export function syncProfile(user: User) {
  const p = currentProfile();
  if (!p) return;
  p.username = user.username;
  p.avatarId = user.avatar_id ?? null;
  persist();
}

/** Un perfil guardado cambio (avatar elegido en "Who's watching?" con su propia sesion). */
export function updateProfile(p: Profile, user: User) {
  p.username = user.username;
  p.avatarId = user.avatar_id ?? null;
  persist();
}

/** La sesion activa ya no sirve (401): ese perfil se va. */
export function dropCurrentProfile() {
  const t = activeToken;
  if (!t) return;
  activeToken = null;
  const before = profiles.list.length;
  profiles.list = profiles.list.filter((p) => p.token !== t);
  if (profiles.list.length !== before) persist();
}

export function markPicked() {
  try {
    sessionStorage.setItem(PICKED, "1");
  } catch {}
}

/** Al abrir: con mas de un perfil (y en TV), preguntar quien mira. */
export function shouldPick(): boolean {
  if (!tv.on || profiles.list.length < 2) return false;
  try {
    return sessionStorage.getItem(PICKED) !== "1";
  } catch {
    return true;
  }
}

/** Pasar a ese perfil: su token y su server, y la pagina de nuevo desde el inicio. */
export function switchTo(p: Profile) {
  markPicked();
  if (p.token === getToken()) {
    profiles.picking = false;
    return;
  }
  if (pickServer && p.server) useServer(p.server);
  setToken(p.token);
  try {
    // Caches de la cuenta anterior (episodios nuevos, etc.).
    for (const k of Object.keys(sessionStorage)) if (k !== PICKED) sessionStorage.removeItem(k);
  } catch {}
  location.hash = "#/";
  location.reload();
}

/** Quitar un perfil del aparato: cierra su sesion en el server (si se puede) y lo borra. Si era
 *  el activo, queda sin sesion. */
export async function removeProfile(p: Profile) {
  await api.logoutAt(profileBase(p), p.token).catch(() => {});
  profiles.list = profiles.list.filter((x) => x !== p && x.token !== p.token);
  persist();
  if (p.token === getToken()) setToken(null);
}

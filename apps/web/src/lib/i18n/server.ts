// Errores del server en el idioma de la interfaz. El server manda `{"error": "...", "code": "...",
// "params": {...}}` (docs/API.md, "Errors"): se traduce por `code` (la clave es "srv." + el codigo
// en camelCase: `bad_login` -> "srv.badLogin") con `params` para los datos ({n}, {status}, {e}).
// Si no hay codigo, o es uno que esta UI no conoce, se reconoce el texto (los servers anteriores a
// los codigos solo mandan el texto); si tampoco, se muestra tal cual vino.
import { has, t, type Key, type Params } from "../i18n.svelte";

/** Clave de un codigo del server (`sync_failed` -> "srv.syncFailed"), si esta UI la tiene. */
export function codeKey(code: string): Key | null {
  const k = "srv." + code.replace(/_([a-z0-9])/g, (_, c: string) => c.toUpperCase());
  return /^[a-z0-9_]+$/.test(code) && has(k) ? k : null;
}

// ---- servers sin codigos: por el texto en ingles

const EXACT: Record<string, Key> = {
  "Invalid username or password": "srv.badLogin",
  "Invalid username or password (offline: sign in once while the server is reachable)": "srv.badLoginOffline",
  "This account is disabled": "srv.disabled",
  "Session expired, please log in again": "srv.expired",
  "Missing session token": "srv.noToken",
  "The server rejected that token": "srv.badToken",
  "Admin access required": "srv.adminOnly",
  "Admin is not available in local mode": "srv.adminLocal",
  "Username already exists": "srv.userExists",
  "User not found": "srv.userNotFound",
  "Title not found": "srv.titleNotFound",
  "Avatar not found": "srv.avatarNotFound",
  "Session not found": "srv.sessionNotFound",
  "unknown or expired media session": "srv.mediaSession",
  "Unknown API route": "srv.unknownRoute",
  "Unknown stream, reload the stream list": "srv.unknownStream",
  "Torrentio is unreachable right now, try again in a moment": "srv.torrentioDown",
  "Torrentio sent an invalid response": "srv.torrentioBad",
  "The catalog service (Cinemeta) is unreachable, try again in a moment": "srv.cinemetaDown",
  "The catalog service (Cinemeta) sent an invalid response": "srv.cinemetaBad",
  "The subtitles service (OpenSubtitles) is unreachable": "srv.subsDown",
  "The subtitles service sent an invalid response": "srv.subsBad",
  "Could not download the subtitle": "srv.subDownload",
  "Subtitle file is too large": "srv.subTooLarge",
  "Subtitle file is corrupted": "srv.subCorrupt",
  "subtitle not found": "srv.subNotFound",
  "Could not download the image": "srv.imgDownload",
  "Image is too large": "srv.imgTooLarge",
  "Password must be at least 6 characters": "srv.pwShort",
  "Password is too long": "srv.pwLong",
  "Current password is incorrect": "srv.pwWrong",
  "Username must be 1-32 characters: letters, numbers, '_', '-' or '.'": "srv.badUsername",
  "You can't remove your own admin rights": "srv.selfAdmin",
  "You can't disable your own account": "srv.selfDisable",
  "You can't delete your own account": "srv.selfDelete",
  "You can't delete the last admin": "srv.lastAdmin",
  "There must be at least one active admin": "srv.needAdmin",
  "Invalid Real-Debrid API key": "srv.rdInvalid",
  "Real-Debrid key is not set": "srv.rdMissing",
  "Only Torrentio / Real-Debrid links can be played": "srv.onlyTorrentio",
  "Server URL must be http(s)://host[:port]": "srv.serverUrl",
  "Invalid server URL": "srv.badServerUrl",
  "Change your username on the server; this device picks it up when it syncs": "srv.renameOnServer",
  "Change your password on the server; this device picks it up on the next online sign-in": "srv.pwOnServer",
  "That server session belongs to a different account": "srv.otherAccount",
  "Search query is too long": "srv.queryLong",
  "transcoding is not available on this device (ffmpeg not found); use direct play": "srv.noFfmpeg",
  "stream is still buffering, try again": "srv.buffering",
  "Internal server error": "srv.internal",
};

/** Los que llevan un dato: el primer grupo va como el parametro que se indica. */
const PATTERNS: [RegExp, Key, string][] = [
  [/^Torrentio answered (\d+)/, "srv.torrentioStatus", "status"],
  [/^The catalog service \(Cinemeta\) answered (\d+)/, "srv.cinemetaStatus", "status"],
  [/^The subtitles service answered (\d+)/, "srv.subsStatus", "status"],
  [/^The gallery is full \((\d+) images\)\. Delete some first\.$/, "srv.galleryFull", "n"],
  [/^Pick up to (\d+) genres/, "srv.maxGenres", "n"],
  [/^Too many failed login attempts\. Try again in (\d+) minute/, "srv.tooManyLogins", "n"],
  [/^Sync failed: (.*)$/s, "srv.syncFailed", "e"],
  [/No space left on device|ENOSPC|ran out of disk space/, "srv.noSpace", "_"],
];

/** El mensaje del server en el idioma de la interfaz: por `code` si viene (y se conoce), si no por
 *  el texto, y si tampoco, tal cual. */
export function serverText(msg: string, code?: string | null, params?: Record<string, unknown> | null): string {
  const k = code ? codeKey(code) : null;
  if (k) {
    const p: Params = {};
    for (const [name, v] of Object.entries(params ?? {})) if (typeof v === "string" || typeof v === "number") p[name] = v;
    return t(k, p);
  }
  const exact = EXACT[msg];
  if (exact) return t(exact);
  for (const [re, key, param] of PATTERNS) {
    const m = re.exec(msg);
    if (m) return t(key, { [param]: m[1] ?? "" });
  }
  return msg;
}

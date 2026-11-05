// Errores del server (`{"error": "..."}`, en inglés) que la UI muestra traducidos. El server no
// manda codigos: se reconoce el texto (exacto, o con una expresion para los que llevan un numero).
// Los que no estan aca se muestran tal cual vienen. Si cambia un texto en el backend, cambiarlo
// aca tambien (PENDIENTES.md: que el server mande codigos de error).
import { t, type Key } from "../i18n.svelte";

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
};

/** Los que llevan un dato: el primer grupo va como {code}, {n} o {e} segun la clave. */
const PATTERNS: [RegExp, Key, string][] = [
  [/^Torrentio answered (\d+), try again in a moment$/, "srv.torrentioStatus", "code"],
  [/^The catalog service \(Cinemeta\) answered (\d+)$/, "srv.cinemetaStatus", "code"],
  [/^The subtitles service answered (\d+)$/, "srv.subsStatus", "code"],
  [/^The gallery is full \((\d+) images\)\. Delete some first\.$/, "srv.galleryFull", "n"],
  [/^Pick up to (\d+) genres/, "srv.maxGenres", "n"],
  [/^Sync failed: (.*)$/s, "srv.syncFailed", "e"],
  [/No space left on device|ENOSPC/, "srv.noSpace", "_"],
];

/** El mensaje del server en el idioma de la interfaz (o tal cual, si no se conoce). */
export function serverText(msg: string): string {
  const k = EXACT[msg];
  if (k) return t(k);
  for (const [re, key, param] of PATTERNS) {
    const m = re.exec(msg);
    if (m) return t(key, { [param]: m[1] ?? "" });
  }
  return msg;
}

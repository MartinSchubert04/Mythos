// Ver un stream afuera de Mythos: el link directo al archivo (con el token en ?t=, como lo usa el
// <video>) para copiarlo o abrirlo en VLC (desktop: comando `open_in_vlc` de apps/desktop).
import { api, mediaUrl } from "./api";
import { isDesktop } from "./platform";
import type { ClientCaps, PlayInfo, Stream } from "./types";

// Con todo en true el server contesta "direct" y no arranca ffmpeg: VLC lee cualquier cosa.
const ALL: ClientCaps = { h264: true, hevc: true, av1: true, vp9: true, mkv: true, aac: true, ac3: true, eac3: true, dts: true, opus: true, flac: true };

/** El link directo de un stream. Si ya hay una sesion para ese stream (el reproductor), se usa;
 *  si no, se abre una sin `meta_id`: asi el server no cierra la que este reproduciendo. */
export async function streamLink(s: Stream, open?: { key: string; info: PlayInfo } | null): Promise<string> {
  const info =
    open && open.key === s.key
      ? open.info
      : await api.play({ stream_key: s.key, info_hash: s.info_hash, file_idx: s.file_idx, url: s.url, caps: ALL });
  return mediaUrl(info.direct_url);
}

export async function copyText(text: string): Promise<void> {
  try {
    await navigator.clipboard.writeText(text);
    return;
  } catch {}
  // Sin permiso de portapapeles (http sin TLS, WebView viejo): el metodo de siempre.
  const ta = document.createElement("textarea");
  ta.value = text;
  ta.style.cssText = "position:fixed;opacity:0;pointer-events:none";
  document.body.append(ta);
  ta.select();
  const ok = document.execCommand("copy");
  ta.remove();
  if (!ok) throw new Error("Couldn't copy the link");
}

export const canVlc = isDesktop;

/** Abre el link en VLC (solo desktop). Tira con el motivo si no esta instalado. */
export async function openInVlc(url: string): Promise<void> {
  const { invoke } = await import("@tauri-apps/api/core");
  await invoke("open_in_vlc", { url });
}

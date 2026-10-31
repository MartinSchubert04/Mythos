// Que puede reproducir este navegador sin ayuda del server (ClientCaps del contrato).
import { read } from "./store";
import type { ClientCaps, PlayInfo } from "./types";

let memo: ClientCaps | null = null;

export function detectCaps(): ClientCaps {
  if (memo) return { ...memo };
  const v = document.createElement("video");
  const ms = (globalThis as { MediaSource?: typeof MediaSource }).MediaSource;
  const can = (...types: string[]) =>
    types.some((t) => {
      try {
        return v.canPlayType(t) !== "" || !!ms?.isTypeSupported?.(t);
      } catch {
        return false;
      }
    });
  memo = {
    h264: can('video/mp4; codecs="avc1.640028"', 'video/mp4; codecs="avc1.42E01E"'),
    hevc: can('video/mp4; codecs="hvc1.1.6.L120.90"', 'video/mp4; codecs="hev1.1.6.L120.90"'),
    av1: can('video/mp4; codecs="av01.0.08M.08"'),
    vp9: can('video/webm; codecs="vp9"', 'video/mp4; codecs="vp09.00.40.08"'),
    // Solo si el navegador lo dice: si no, el server remuxea (barato) y no hay sorpresas.
    mkv: v.canPlayType("video/x-matroska") !== "" || v.canPlayType("video/mkv") !== "",
    aac: can('audio/mp4; codecs="mp4a.40.2"'),
    ac3: can('audio/mp4; codecs="ac-3"'),
    eac3: can('audio/mp4; codecs="ec-3"'),
    dts: can('audio/mp4; codecs="dtsc"', 'audio/mp4; codecs="dtse"'),
    opus: can('audio/webm; codecs="opus"', 'audio/mp4; codecs="opus"'),
    flac: can('audio/mp4; codecs="flac"', "audio/flac"),
  };
  // Para probar: localStorage "mythos.caps" = '{"hevc":false,"mkv":false}' fuerza remux/transcode.
  try {
    Object.assign(memo, JSON.parse(read("mythos.caps") ?? "{}"));
  } catch {}
  return { ...memo };
}

/** Codec del contrato a la bandera de ClientCaps. */
function videoFlag(codec?: string | null): keyof ClientCaps | null {
  const c = (codec ?? "").toLowerCase();
  if (/hevc|h265|hvc|hev/.test(c)) return "hevc";
  if (/av1|av01/.test(c)) return "av1";
  if (/vp9|vp09/.test(c)) return "vp9";
  if (/h264|avc/.test(c)) return "h264";
  return null;
}
function audioFlag(codec?: string | null): keyof ClientCaps | null {
  const c = (codec ?? "").toLowerCase();
  if (/eac3|e-ac-3|ec-3/.test(c)) return "eac3";
  if (/ac3|ac-3/.test(c)) return "ac3";
  if (/dts/.test(c)) return "dts";
  if (/opus/.test(c)) return "opus";
  if (/flac/.test(c)) return "flac";
  if (/aac|mp4a/.test(c)) return "aac";
  return null;
}

/** Despues de que el modo direct fallo: que bandera apagar para que el server convierta.
 *  Primero el contenedor (mkv), despues el codec de video (salvo h264) y despues el de audio.
 *  Devuelve null si ya no queda nada para apagar. */
export function degrade(caps: ClientCaps, info: PlayInfo, what: "video" | "audio" | "any"): ClientCaps | null {
  const next = { ...caps };
  const mkv = /mkv|matroska|webm/i.test(info.container ?? info.file_name);
  const vf = videoFlag(info.video_codec);
  const af = audioFlag(info.audio_tracks[0]?.codec);
  if (what !== "audio" && mkv && next.mkv) next.mkv = false;
  else if (what !== "audio" && vf && vf !== "h264" && next[vf]) next[vf] = false;
  else if (what !== "video" && af && af !== "aac" && next[af]) next[af] = false;
  else if (what === "video" && vf && next[vf]) next[vf] = false;
  else if (what === "audio" && af && next[af]) next[af] = false;
  else return null;
  return next;
}

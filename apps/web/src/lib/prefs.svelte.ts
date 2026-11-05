// Preferencias de la interfaz. Se guardan en local (para que apliquen al instante al abrir) y en
// el server, dentro de Settings.ui, junto con el tema y las paletas. Volumen y barra lateral son
// solo de este dispositivo. El idioma de la interfaz vive en lib/i18n.svelte.ts y viaja aca como
// `ui.lang`.
import { api } from "./api";
import { i18n, isLang, setLang, t, type Key, type Lang } from "./i18n.svelte";
import { readJson, write } from "./store";
import { allPalettes, applyRemoteTheme, currentTheme } from "./themes";

export type UiPrefs = {
  /** Idioma de subtitulos preferido (ISO 639-2, "eng", "spa"...). Vacio = sin subtitulos. */
  subLang: string;
  /** Poner esos subtitulos tambien cuando el audio ya esta en ese idioma. */
  subAlways: boolean;
  /** Tamaño de los subtitulos en % (100 = normal). */
  subSize: number;
  /** Opacidad del fondo detras de cada linea, en % (0 = sin fondo, solo sombra). */
  subBg: number;
  /** Altura de los subtitulos: margen desde abajo en % del alto del video. */
  subPos: number;
  /** Al terminar un episodio, cuenta regresiva y el siguiente. */
  autoplayNext: boolean;
  /** Idioma de audio preferido ("eng", "spa-419", "spa-ES"...; vacio = el original): elige esa
   *  pista y sube los streams en ese idioma (lib/streamlang.ts). */
  audioLang: string;
  /** Desenfoca las imagenes y esconde la descripcion de los episodios no vistos. */
  hideSpoilers: boolean;
  reduceMotion: boolean;
  /** Desktop y Android: aviso al pasar al server local y al volver (solo de este dispositivo). */
  localNotice: boolean;
  sidebarCollapsed: boolean;
  volume: number;
  muted: boolean;
};

const KEY = "mythos.prefs";
const defaults = (): UiPrefs => ({
  subLang: "spa", // español por defecto, como la interfaz
  subAlways: false,
  subSize: 100,
  subBg: 35,
  subPos: 8,
  autoplayNext: true,
  audioLang: "",
  hideSpoilers: false,
  reduceMotion: false,
  localNotice: true,
  sidebarCollapsed: false,
  volume: 1,
  muted: false,
});

export const prefs = $state<UiPrefs>(readJson(KEY, defaults()));

/** Las que viajan al server. */
const SYNCED = ["subLang", "subAlways", "subSize", "subBg", "subPos", "autoplayNext", "audioLang", "hideSpoilers", "reduceMotion"] as const;

function applyMotion() {
  document.documentElement.toggleAttribute("data-reduce-motion", prefs.reduceMotion);
}
applyMotion();

let pushTimer = 0;
/** Guarda en local enseguida y en el server un momento despues (agrupa cambios seguidos). */
export function savePrefs(sync = true) {
  write(KEY, JSON.stringify(prefs));
  applyMotion();
  if (!sync) return;
  clearTimeout(pushTimer);
  pushTimer = window.setTimeout(() => {
    api.saveSettings({ ui: uiPayload() }).catch(() => {});
  }, 600);
}

function uiPayload(): Record<string, unknown> {
  const out: Record<string, unknown> = { theme: currentTheme(), palettes: allPalettes() };
  for (const k of SYNCED) out[k] = prefs[k];
  out.lang = i18n.lang;
  return out;
}

/** Lo que vino del server al entrar pisa lo local. */
export function applyRemoteUi(ui: Record<string, unknown>) {
  const d = defaults();
  for (const k of SYNCED) {
    const v = ui[k];
    if (v !== undefined && typeof v === typeof d[k]) (prefs as unknown as Record<string, unknown>)[k] = v;
  }
  if (ui.theme) applyRemoteTheme(ui.theme, ui.palettes);
  if (isLang(ui.lang)) setLang(ui.lang);
  savePrefs(false);
}

/** Cambia el idioma de la interfaz y lo guarda (local y en el server). */
export function setUiLang(l: Lang) {
  setLang(l);
  savePrefs();
}

// ---- idiomas

/** Opcion de una lista de idiomas con el nombre traducido: `label` es un getter, asi la lista es
 *  fija pero el texto sigue al idioma de la interfaz. */
export function langOpt<V extends string>(value: V, key: Key): { value: V; label: string } {
  return {
    value,
    get label() {
      return t(key);
    },
  };
}

export const LANGS: { value: string; label: string }[] = [
  langOpt("", "lang.off"),
  langOpt("eng", "lang.eng"),
  langOpt("spa", "lang.spa"),
  langOpt("por", "lang.por"),
  langOpt("fre", "lang.fre"),
  langOpt("ger", "lang.ger"),
  langOpt("ita", "lang.ita"),
  langOpt("dut", "lang.dut"),
  langOpt("pol", "lang.pol"),
  langOpt("rus", "lang.rus"),
  langOpt("tur", "lang.tur"),
  langOpt("ara", "lang.ara"),
  langOpt("jpn", "lang.jpn"),
  langOpt("kor", "lang.kor"),
  langOpt("chi", "lang.chi"),
];

const ALIAS: Record<string, string> = {
  en: "eng", es: "spa", esp: "spa", spl: "spa", spn: "spa", ea: "spa", pt: "por", pob: "por", pb: "por", ptbr: "por",
  fr: "fre", fra: "fre", de: "ger", deu: "ger", it: "ita", nl: "dut", nld: "dut", pl: "pol", ru: "rus",
  tr: "tur", ar: "ara", ja: "jpn", ko: "kor", zh: "chi", zho: "chi", ze: "chi",
};

/** Codigo de idioma normalizado ("es", "spa", "es-419" → "spa"). */
export function normLang(l?: string | null): string {
  if (!l) return "";
  const s = l.toLowerCase().replace(/[^a-z]/g, "").slice(0, 4);
  return ALIAS[s] ?? ALIAS[s.slice(0, 2)] ?? s.slice(0, 3);
}

export function langName(l?: string | null): string {
  const n = normLang(l);
  return LANGS.find((x) => x.value === n)?.label ?? (l ? l.toUpperCase() : t("lang.unknown"));
}

// Codigos de OpenSubtitles / ffprobe → BCP 47, para el nombre del idioma en su propio idioma.
const BCP: Record<string, string> = {
  eng: "en", en: "en", spa: "es", es: "es", spl: "es-419", ea: "es-419", spn: "es-419", por: "pt", pt: "pt",
  pob: "pt-BR", pb: "pt-BR", fre: "fr", fra: "fr", fr: "fr", ger: "de", deu: "de", de: "de", ita: "it", it: "it",
  dut: "nl", nld: "nl", nl: "nl", pol: "pl", pl: "pl", rus: "ru", ru: "ru", tur: "tr", tr: "tr", ara: "ar", ar: "ar",
  jpn: "ja", ja: "ja", kor: "ko", ko: "ko", chi: "zh-Hans", zho: "zh-Hans", zh: "zh-Hans", zht: "zh-Hant", zhe: "zh-Hant",
  swe: "sv", sv: "sv", cze: "cs", ces: "cs", cs: "cs", dan: "da", da: "da", fin: "fi", fi: "fi", nor: "nb", no: "nb",
  gre: "el", ell: "el", el: "el", heb: "he", he: "he", hun: "hu", hu: "hu", rum: "ro", ron: "ro", ro: "ro", hrv: "hr",
  hr: "hr", srp: "sr", sr: "sr", bul: "bg", bg: "bg", ukr: "uk", uk: "uk", ind: "id", id: "id", vie: "vi", vi: "vi",
  tha: "th", th: "th", per: "fa", fas: "fa", fa: "fa", hin: "hi", hi: "hi", slo: "sk", slk: "sk", sk: "sk", slv: "sl",
  sl: "sl", est: "et", lav: "lv", lit: "lt", cat: "ca", ca: "ca", baq: "eu", eus: "eu", glg: "gl", may: "ms", msa: "ms",
  mac: "mk", mkd: "mk", mk: "mk", alb: "sq", sqi: "sq", sq: "sq", ice: "is", isl: "is", is: "is", bos: "bs", bs: "bs",
  geo: "ka", kat: "ka", arm: "hy", hye: "hy", ben: "bn", tam: "ta", tel: "te", urd: "ur", mal: "ml", sin: "si",
};

/** "Español (Latinoamérica)", "Português (Brasil)", "English"... (el nombre en ese idioma). */
export function nativeLangName(code?: string | null): string {
  const raw = (code ?? "").toLowerCase().replace(/[^a-z-]/g, "");
  const tag = BCP[raw] ?? BCP[raw.slice(0, 3)] ?? BCP[raw.slice(0, 2)];
  if (tag) {
    try {
      const n = new Intl.DisplayNames([tag], { type: "language" }).of(tag);
      if (n) return n.charAt(0).toLocaleUpperCase(tag) + n.slice(1);
    } catch {}
  }
  return langName(code);
}

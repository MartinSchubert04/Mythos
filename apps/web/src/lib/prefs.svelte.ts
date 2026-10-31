// Preferencias de la interfaz. Se guardan en local (para que apliquen al instante al abrir) y en
// el server, dentro de Settings.ui, junto con el tema y las paletas. Volumen y barra lateral son
// solo de este dispositivo.
import { api } from "./api";
import { readJson, write } from "./store";
import { allPalettes, applyRemoteTheme, currentTheme } from "./themes";

export type UiPrefs = {
  /** Idioma de subtitulos preferido (ISO 639-2, "eng", "spa"...). Vacio = sin subtitulos. */
  subLang: string;
  /** Tamaño de los subtitulos en % (100 = normal). */
  subSize: number;
  /** Al terminar un episodio, cuenta regresiva y el siguiente. */
  autoplayNext: boolean;
  reduceMotion: boolean;
  sidebarCollapsed: boolean;
  volume: number;
  muted: boolean;
};

const KEY = "mythos.prefs";
const defaults = (): UiPrefs => ({
  subLang: "eng",
  subSize: 100,
  autoplayNext: true,
  reduceMotion: false,
  sidebarCollapsed: false,
  volume: 1,
  muted: false,
});

export const prefs = $state<UiPrefs>(readJson(KEY, defaults()));

/** Las que viajan al server. */
const SYNCED = ["subLang", "subSize", "autoplayNext", "reduceMotion"] as const;

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
  savePrefs(false);
}

// ---- idiomas

export const LANGS: { value: string; label: string }[] = [
  { value: "", label: "Off" },
  { value: "eng", label: "English" },
  { value: "spa", label: "Spanish" },
  { value: "por", label: "Portuguese" },
  { value: "fre", label: "French" },
  { value: "ger", label: "German" },
  { value: "ita", label: "Italian" },
  { value: "dut", label: "Dutch" },
  { value: "pol", label: "Polish" },
  { value: "rus", label: "Russian" },
  { value: "tur", label: "Turkish" },
  { value: "ara", label: "Arabic" },
  { value: "jpn", label: "Japanese" },
  { value: "kor", label: "Korean" },
  { value: "chi", label: "Chinese" },
];

const ALIAS: Record<string, string> = {
  en: "eng", es: "spa", esp: "spa", spl: "spa", ea: "spa", pt: "por", pob: "por", pb: "por", ptbr: "por",
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
  return LANGS.find((x) => x.value === n)?.label ?? (l ? l.toUpperCase() : "Unknown");
}

// Idioma de la interfaz: español por defecto, inglés como opción (Settings → Appearance).
//
// - Los textos viven en lib/i18n/es/*.ts y lib/i18n/en/*.ts, un archivo por zona de la interfaz
//   (common, browse, player, account, admin), con claves planas con prefijo ("player.resume").
//   El español es la fuente de las claves: a un archivo de inglés que le falte (o le sobre) una
//   clave, TypeScript lo marca.
// - `t("clave", { n: 3, name: "Ana" })`: reemplaza `{n}`, `{name}`. Si el texto es un plural
//   (`{ one, other }`), elige con `n` (1 = one; es y en no necesitan mas).
// - Reactivo: `t()` lee `i18n.lang` ($state), asi que cambiar de idioma repinta lo que lo usa sin
//   recargar. Los diccionarios son objetos fijos (no se arma nada en cada render) y los formatos de
//   Intl se crean una vez por idioma y opciones.
// - Lo que se arma fuera de un componente (listas de opciones, avisos) tiene que llamar a `t()` al
//   usarse, no al cargar el modulo: si no, queda en el idioma con el que arranco.
// - Se guarda en localStorage (`mythos.lang`, lo lee tambien el aviso de navegador viejo de
//   index.html) y viaja al server con el resto de las preferencias (prefs.svelte.ts, `ui.lang`).
import { read, write } from "./store";
import en from "./i18n/en";
import es from "./i18n/es";
import type { Msg, Plural } from "./i18n/types";

export type Lang = "es" | "en";
export type Key = keyof typeof es;
export type Params = Record<string, string | number>;

export const LANG_KEY = "mythos.lang";
export const UI_LANGS: { value: Lang; label: string }[] = [
  { value: "es", label: "Español" },
  { value: "en", label: "English" },
];

const DICTS: Record<Lang, Record<Key, Msg>> = { es, en };

export const isLang = (v: unknown): v is Lang => v === "es" || v === "en";

export const i18n = $state<{ lang: Lang }>({ lang: isLang(read(LANG_KEY)) ? (read(LANG_KEY) as Lang) : "es" });

function applyHtmlLang() {
  if (typeof document !== "undefined") document.documentElement.lang = i18n.lang;
}
applyHtmlLang();

/** Cambia el idioma de la interfaz (y lo guarda en este dispositivo). Para que viaje al server,
 *  `setUiLang` de prefs.svelte.ts. */
export function setLang(l: Lang) {
  if (!isLang(l)) return;
  i18n.lang = l;
  write(LANG_KEY, l);
  applyHtmlLang();
}

const PARAM = /\{(\w+)\}/g;

/** El texto de `key` en el idioma actual. */
export function t(key: Key, params?: Params): string {
  const m: Msg | undefined = DICTS[i18n.lang][key] ?? es[key];
  if (m === undefined) return key;
  const s = typeof m === "string" ? m : pick(m, params?.n);
  return params ? s.replace(PARAM, (all, k: string) => (params[k] === undefined ? all : String(params[k]))) : s;
}

function pick(p: Plural, n: string | number | undefined): string {
  return Number(n) === 1 ? p.one : p.other;
}

/** Si existe la clave (para textos que se arman con un codigo: `has(\`lang.${code}\`)`). */
export function has(key: string): key is Key {
  return key in es;
}

// ---- formatos con Intl (uno por idioma y opciones, se reusan)

const locale = () => (i18n.lang === "es" ? "es-AR" : "en-US");
const cache = new Map<string, Intl.NumberFormat | Intl.DateTimeFormat | Intl.RelativeTimeFormat>();
function cached<T extends Intl.NumberFormat | Intl.DateTimeFormat | Intl.RelativeTimeFormat>(kind: string, opts: object, make: (l: string) => T): T {
  const l = locale();
  const k = `${kind}|${l}|${JSON.stringify(opts)}`;
  let f = cache.get(k) as T | undefined;
  if (!f) cache.set(k, (f = make(l)));
  return f;
}

/** 1.234 / 1,234 (y decimales con la coma o el punto que toca). */
export function num(n: number, opts: Intl.NumberFormatOptions = {}): string {
  return cached("n", opts, (l) => new Intl.NumberFormat(l, opts)).format(n);
}

/** Fecha u hora con las opciones de Intl ("12 de oct. de 2024", "Oct 12, 2024"). */
export function date(d: Date | string | number, opts: Intl.DateTimeFormatOptions = { dateStyle: "medium" }): string {
  const v = d instanceof Date ? d : new Date(d);
  if (!isFinite(v.getTime())) return "";
  return cached("d", opts, (l) => new Intl.DateTimeFormat(l, opts)).format(v);
}

/** "hace 5 min" / "5 min ago" desde una fecha. */
export function ago(d: Date | string | number): string {
  const v = d instanceof Date ? d : new Date(d);
  const s = (Date.now() - v.getTime()) / 1000;
  if (!isFinite(s)) return "";
  if (s < 60) return t("time.justNow");
  const rtf = cached("r", { numeric: "always", style: "short" }, (l) => new Intl.RelativeTimeFormat(l, { numeric: "always", style: "short" }));
  if (s < 3600) return rtf.format(-Math.floor(s / 60), "minute");
  if (s < 86400) return rtf.format(-Math.floor(s / 3600), "hour");
  return rtf.format(-Math.floor(s / 86400), "day");
}

/** Duracion corta: "1 h 20 min", "45 min", "30 s" (igual en los dos idiomas, cambia el "y"). */
export function dur(seconds: number): string {
  const s = Math.max(0, Math.round(seconds));
  if (s < 60) return t("time.sec", { n: s });
  const h = Math.floor(s / 3600);
  const m = Math.round((s % 3600) / 60);
  if (!h) return t("time.min", { n: m });
  return m ? t("time.hMin", { h, m }) : t("time.h", { n: h });
}

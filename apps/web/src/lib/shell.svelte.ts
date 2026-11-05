// Android (apps/mobile): lo que hace la interfaz cuando corre dentro del APK. Todo pasa por los
// comandos del plugin `mythos` (apps/mobile/plugin); si alguno no existe (un APK mas viejo que
// esta interfaz), no pasa nada.
//
// - La interfaz principal es la que viene en el APK (habla con el server por `serverBase()`, o
//   mas adelante con el server embebido del modo local) y se actualiza con el APK. Opcional
//   (Settings → App, apagado por defecto): usar la interfaz que sirve el server (`toServer`),
//   que se actualiza con cada deploy sin reinstalar.
// - Boton atras: el mismo camino que Esc (cierra menus, cajones, dialogos y sale del
//   reproductor); sin nada abierto vuelve a la vista anterior, y en el inicio manda la app al
//   fondo.
// - Pantalla encendida mientras se reproduce, iconos de las barras segun el tema, y las
//   actualizaciones del APK (Settings → App).
//
// Desktop (apps/desktop): solo las actualizaciones (src-tauri/src/updater.rs, mismos comandos sin
// el prefijo del plugin) y volver a la misma pantalla despues de "Update".
import { getToken } from "./api";
import { t } from "./i18n.svelte";
import { androidFs, isAndroid, isDesktop, isRemoteShell, setFullscreen, shellInvoke } from "./platform";
import { go, route } from "./router.svelte";
import { read, write } from "./store";
import { toast } from "./toast.svelte";

export type UpdateInfo = { version: string; status: "downloading" | "ready"; progress: number };

export const shell = $state({
  /** Version de la app ("" en la web o mientras no se sabe). */
  version: "",
  update: null as UpdateInfo | null,
});

const ROUTE_KEY = "mythos.route";
/** "1": la pagina del APK abre la interfaz del server (opcional, apagado por defecto). */
const REMOTE_UI_KEY = "mythos.remoteUi";
/** Donde estaba al cerrarse (se lee antes de que la ruta de este arranque lo pise). */
const lastRoute = read(ROUTE_KEY);

export const remoteUi = {
  get on() {
    return isRemoteShell || read(REMOTE_UI_KEY) === "1";
  },
};

/** Prender: guarda la preferencia y navega ya. Apagar (desde la interfaz del server): vuelve a la
 *  pagina del APK, que la borra (ver `initShell`). */
export async function setRemoteUi(on: boolean, url: string): Promise<boolean> {
  if (!on) {
    if (isRemoteShell) leaveServer("#/settings?ui=bundled");
    else write(REMOTE_UI_KEY, null);
    return true;
  }
  write(REMOTE_UI_KEY, "1");
  // Con la sesion de aca (la pagina del server tiene su propio almacenamiento), a Settings.
  if (await toServer(url, getToken() ?? undefined, true, "#/settings")) return true;
  write(REMOTE_UI_KEY, null);
  return false;
}

/** La interfaz del server sabe correr dentro del APK (index.html trae `<meta name="mythos-shell">`).
 *  Un server viejo no: ahi se sigue con la pagina local, que funciona igual que el desktop. */
async function serverHasShellUi(url: string): Promise<boolean> {
  try {
    const r = await fetch(url + "/", { signal: AbortSignal.timeout(5000), cache: "no-store" });
    return r.ok && /name="mythos-shell"/.test(await r.text());
  } catch {
    return false;
  }
}

/** Pagina local → la interfaz del server, si contesta. `token`: el de un login recien hecho
 *  aca (la pagina del server tiene su propio almacenamiento y lo recibe por el plugin). Con
 *  `#/server` en la URL (vino de "Change server") se queda aca, salvo `force`. */
export async function toServer(url: string, token?: string, force = false, to?: string): Promise<boolean> {
  if (!isAndroid || isRemoteShell || !url || read(REMOTE_UI_KEY) !== "1") return false;
  if (!force && location.hash.startsWith("#/server")) return false;
  if (!(await serverHasShellUi(url))) return false;
  try {
    await shellInvoke("open_server", { url, token: token ?? null, route: to ?? (force ? null : lastRoute) });
    return true;
  } catch {
    return false;
  }
}

/** Desde la interfaz del server, a la pagina del APK. Por defecto "Change server" (#/server: el
 *  login con el campo abierto). */
export function leaveServer(to = "#/server") {
  shellInvoke("leave_server", { route: to }).catch(() => {});
}

/** Al arrancar en Android: si el APK se acaba de actualizar, volver a donde estaba. En la interfaz
 *  del server, ademas, el token del login hecho en la pagina del APK. */
export async function takeHandoff(): Promise<string | null> {
  if (!isAndroid) return null;
  try {
    const h = await shellInvoke<{ token: string | null; updated: boolean; version: string }>("handoff");
    shell.version = h.version;
    if (h.updated) restoreRoute();
    return h.token;
  } catch {
    return null;
  }
}

/** Despues de una actualizacion: la misma pantalla y, en el reproductor, el mismo segundo. */
function restoreRoute() {
  const last = lastRoute;
  if (!last || !last.startsWith("#/") || last === "#/") return;
  let path = last.slice(1);
  if (path.startsWith("/watch/") && !/[?&]resume=/.test(path)) path += (path.includes("?") ? "&" : "?") + "resume=1";
  go(path, { replace: true });
}

/** Un comando de la app: en Android del plugin `mythos`, en desktop de la app misma. */
async function appInvoke<T>(cmd: string, args?: Record<string, unknown>): Promise<T> {
  if (isAndroid) return shellInvoke<T>(cmd, args);
  const { invoke } = await import("@tauri-apps/api/core");
  return invoke<T>(cmd, args);
}

export async function checkUpdate(): Promise<string | null> {
  return appInvoke<string | null>("update_check");
}

/** `false`: en Android, antes hay que dar el permiso "Instalar apps desconocidas" (se abrio ese
 *  ajuste); en desktop, no habia nada listo. Si instala, la app se reinicia. */
export async function installUpdate(): Promise<boolean> {
  return appInvoke<boolean>("update_install");
}

/** Las actualizaciones: estado al arrancar y aviso cuando una queda lista. */
function watchUpdates(readyText: (v: string) => string) {
  appInvoke<UpdateInfo | null>("update_state").then((u) => (shell.update = u), () => {});
  import("@tauri-apps/api/event")
    .then((m) =>
      m.listen<UpdateInfo | null>("mythos://update", (e) => {
        const was = shell.update?.status;
        shell.update = e.payload;
        if (e.payload?.status === "ready" && was !== "ready") toast(readyText(e.payload.version));
      }),
    )
    .catch(() => {});
}

/** Donde esta, para volver ahi despues de una actualizacion. */
function trackRoute() {
  $effect.root(() => {
    $effect(() => {
      const q = route.query.toString();
      write(ROUTE_KEY, "#" + route.path + (q ? "?" + q : ""));
    });
  });
}

function initDesktop() {
  import("@tauri-apps/api/app")
    .then((m) => m.getVersion())
    .then((v) => (shell.version = v), () => {});
  trackRoute();
  appInvoke<boolean>("update_handoff").then((updated) => updated && restoreRoute(), () => {});
  watchUpdates((v) => t("account.app.readyDesktopToast", { v }));
}

// ---- boton atras

function onBack() {
  // Lo que este abierto se cierra con el mismo Esc de siempre. Si el reproductor esta en
  // pantalla completa, Esc la cerraria: en Android "atras" sale del reproductor directamente
  // (androidFs.backPress, ver setFullscreen en platform.ts).
  const target = (document.activeElement as HTMLElement | null) ?? document.body;
  const ev = new KeyboardEvent("keydown", { key: "Escape", code: "Escape", bubbles: true, cancelable: true });
  androidFs.backPress = true;
  try {
    target.dispatchEvent(ev);
  } finally {
    androidFs.backPress = false;
  }
  if (ev.defaultPrevented || route.view === "player") return;
  if (history.state !== "root" && history.length > 1) history.back();
  else if (route.view !== "home") go("/", { replace: true });
  else shellInvoke("minimize").catch(() => {});
}

// ---- reproduciendo: pantalla encendida y nada de instalar

let playing = false;
function syncPlaying() {
  const now = [...document.querySelectorAll("video")].some((v) => !v.paused && !v.ended);
  if (now === playing) return;
  playing = now;
  shellInvoke("set_playing", { playing }).catch(() => {});
}

// ---- iconos de las barras del sistema: oscuros en temas claros

let barsLight: boolean | null = null;
function syncBars() {
  const light = route.view !== "player" && getComputedStyle(document.documentElement).colorScheme.includes("light");
  if (light === barsLight) return;
  barsLight = light;
  shellInvoke("set_bars", { light }).catch(() => {});
}

let wasPlayer = false;

export function initShell() {
  if (isDesktop) return initDesktop();
  if (!isAndroid) return;

  import("@tauri-apps/api/app")
    .then((m) => {
      m.onBackButtonPress(onBack).catch(() => {});
    })
    .catch(() => {});

  for (const e of ["play", "playing", "pause", "ended", "emptied"]) document.addEventListener(e, syncPlaying, true);

  new MutationObserver(syncBars).observe(document.documentElement, { attributes: true, attributeFilter: ["data-theme", "data-palette"] });

  // Se apago "interfaz del server" desde ella: la pagina del APK se queda.
  if (!isRemoteShell && route.query.get("ui") === "bundled") {
    write(REMOTE_UI_KEY, null);
    history.replaceState(history.state, "", "#" + route.path);
    route.query = new URLSearchParams();
  }

  trackRoute();
  $effect.root(() => {
    $effect(() => {
      // El reproductor: pantalla completa (horizontal, sin barras) al entrar, normal al salir.
      const inPlayer = route.view === "player";
      if (inPlayer !== wasPlayer) {
        wasPlayer = inPlayer;
        setFullscreen(document.body, inPlayer);
      }
      setTimeout(() => {
        syncPlaying();
        syncBars();
      });
    });
  });

  watchUpdates((v) => t("account.app.readyAndroidToast", { v }));
}

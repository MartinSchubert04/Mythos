import { mount } from "svelte";
// Fuentes: el navegador baja cada una recien cuando un tema la usa (Inter es la de casi todos).
import "@fontsource-variable/inter/wght.css";
import "@fontsource-variable/geist-mono/wght.css";
import "@fontsource/jacquard-24/latin-400.css";
import "@fontsource/jetbrains-mono/latin-400.css";
import "@fontsource/jetbrains-mono/latin-700.css";
import "./styles/themes.css";
import "./styles/palettes.css";
import "./styles/base.css";
import "./styles/athanor.css";
import "./styles/app.css";
import "./styles/tv.css";
import App from "./App.svelte";
import { initGlass } from "./lib/glass.svelte";
import { applyLite } from "./lib/perf";
import { isDesktop } from "./lib/platform";
import { initShell } from "./lib/shell.svelte";
import { initSpatial } from "./lib/spatial.svelte";
import { initTizen } from "./lib/tizen";
import { initTv } from "./lib/tv.svelte";
import { installTooltips } from "./lib/tooltip";

async function boot() {
  // Modo mock (solo desarrollo, sin server). En el build de produccion esta rama no existe.
  if (import.meta.env.DEV && import.meta.env.VITE_MOCK === "1") {
    const { installMock } = await import("./lib/mock");
    installMock();
  }
  applyLite();
  initShell();
  // Modo TV (control remoto): se averigua de fondo; la navegacion con flechas espera a saberlo.
  initTv();
  initSpatial();
  // Samsung: las teclas multimedia del control (dentro de la app de la tele).
  initTizen();
  initGlass();
  mount(App, { target: document.getElementById("app")! });
  installTooltips();
  // Desktop: la ventana nace oculta (ver apps/desktop/src-tauri/src/lib.rs) y se muestra con el
  // primer cuadro ya pintado.
  if (isDesktop)
    requestAnimationFrame(() =>
      import("@tauri-apps/api/window")
        .then(async (m) => {
          const w = m.getCurrentWindow();
          await w.show();
          await w.setFocus();
        })
        .catch(() => {}),
    );
}

boot();

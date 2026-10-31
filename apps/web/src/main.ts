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
import App from "./App.svelte";
import { installTooltips } from "./lib/tooltip";

async function boot() {
  // Modo mock (solo desarrollo, sin server). En el build de produccion esta rama no existe.
  if (import.meta.env.DEV && import.meta.env.VITE_MOCK === "1") {
    const { installMock } = await import("./lib/mock");
    installMock();
  }
  mount(App, { target: document.getElementById("app")! });
  installTooltips();
}

boot();

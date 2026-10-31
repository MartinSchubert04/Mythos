import { defineConfig, type Plugin } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// La interfaz de Mythos. En desarrollo /api va al server local (cargo run -p mythos).
// Con --mode mock (o VITE_MOCK=1) no hace falta server: ver src/lib/mock/.
/** El <script type="module"> de la app no va en el HTML: lo carga `mythosBoot` (index.html) solo
 *  si el navegador o WebView alcanza la version minima. En uno viejo el modulo tiraria un error de
 *  sintaxis y quedaria la pantalla en blanco; asi se ve el aviso "Update Android System WebView".
 *  El de Vite en desarrollo (/@vite/client) queda como esta. */
function gate(): Plugin {
  return {
    name: "mythos-webview-gate",
    transformIndexHtml: {
      order: "post",
      handler(html) {
        let n = 0;
        const out = html.replace(/<script type="module"(?: crossorigin)? src="([^"]+)"><\/script>/g, (tag, src: string) => {
          if (src.startsWith("/@")) return tag;
          n++;
          return `<script>mythosBoot(${JSON.stringify(src)})</script>`;
        });
        if (n !== 1) throw new Error(`gate: se esperaba un script de entrada y hay ${n}`);
        return out;
      },
    },
  };
}

export default defineConfig({
  plugins: [svelte(), gate()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: { "/api": { target: "http://127.0.0.1:7700", changeOrigin: false } },
  },
  preview: { port: 5173, proxy: { "/api": "http://127.0.0.1:7700" } },
  build: {
    // Minimo soportado: Chromium 111 (color-mix). Lo mas viejo ve el aviso de index.html.
    target: ["es2022", "chrome111"],
    // Sin el polyfill de modulepreload: todos los navegadores que importan aca lo traen.
    modulePreload: { polyfill: false },
    chunkSizeWarningLimit: 600,
  },
});

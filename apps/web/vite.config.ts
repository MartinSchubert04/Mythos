import { defineConfig } from "vite";
import { svelte } from "@sveltejs/vite-plugin-svelte";

// La interfaz de Mythos. En desarrollo /api va al server local (cargo run -p mythos).
// Con --mode mock (o VITE_MOCK=1) no hace falta server: ver src/lib/mock/.
export default defineConfig({
  plugins: [svelte()],
  clearScreen: false,
  server: {
    port: 5173,
    strictPort: true,
    proxy: { "/api": { target: "http://127.0.0.1:7700", changeOrigin: false } },
  },
  preview: { port: 5173, proxy: { "/api": "http://127.0.0.1:7700" } },
  build: {
    target: "es2022",
    // Sin el polyfill de modulepreload: todos los navegadores que importan aca lo traen.
    modulePreload: { polyfill: false },
    chunkSizeWarningLimit: 600,
  },
});

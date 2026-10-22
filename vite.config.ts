import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import { fileURLToPath, URL } from "node:url";
import { frontendBuild } from "./scripts/version.mjs";
export default defineConfig({
  define: { __APP_BUILD__: JSON.stringify(frontendBuild()) },
  plugins: [react()],
  resolve: { alias: { "@": fileURLToPath(new URL("./src", import.meta.url)) } },
  server: {
    port: 1420,
    strictPort: true,
    proxy: { "/api": "http://127.0.0.1:8788" },
    watch: { ignored: ["**/target/**", "**/crates/**", "**/src-tauri/**"] },
  },
});

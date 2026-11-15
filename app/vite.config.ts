import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import pkg from "./package.json";

export default defineConfig({
  plugins: [react()],
  // the app's version, shown in the File menu (kept equal to tauri.conf.json's)
  define: { __APP_VERSION__: JSON.stringify(pkg.version) },
  clearScreen: false,
  // templates import ../examples from the repo root
  server: { port: 5174, strictPort: true, host: "127.0.0.1", fs: { allow: [".."] } },
});

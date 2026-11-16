import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built files from dist/; in development it loads the Vite
// dev server. The app needs no network access, so nothing is fetched at run time.
export default defineConfig({
  plugins: [react()],
  clearScreen: false,
  server: { port: 5173, strictPort: true },
  build: { target: "es2021", outDir: "dist", emptyOutDir: true },
  test: { environment: "jsdom", globals: true },
});

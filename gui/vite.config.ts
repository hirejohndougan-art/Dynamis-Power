import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Tauri serves the built files from dist/; in development it loads the Vite
// dev server. The app needs no network access, so nothing is fetched at run time.
// __STAND_IN__: whether the in-memory stand-in backend is bundled. Only for
// the development server and tests; the app the person installs leaves it
// out, so a slow laptop reads less script when the window opens.
export default defineConfig(({ command, mode }) => ({
  plugins: [react()],
  clearScreen: false,
  define: { __STAND_IN__: JSON.stringify(command === "serve" || mode === "test") },
  server: { port: 5173, strictPort: true },
  build: { target: "es2021", outDir: "dist", emptyOutDir: true },
  test: { environment: "jsdom", globals: true },
}));

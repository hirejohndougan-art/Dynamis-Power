import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// The clickable demo (demo.html): the app on its stand-in backend inside a
// pretend desktop. Built into demo-dist/ and then made one self-contained
// file by scripts/inline-demo.mjs, for sharing as a single page.
export default defineConfig({
  plugins: [react()],
  // The demo runs on the stand-in backend.
  define: { __STAND_IN__: "true" },
  build: {
    target: "es2021",
    outDir: "demo-dist",
    emptyOutDir: true,
    assetsInlineLimit: 1_000_000,
    rollupOptions: { input: "demo.html", output: { inlineDynamicImports: true } },
  },
});

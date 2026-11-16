// Turn demo-dist/demo.html and its built script and styles into one file:
// demo-dist/dynamis-power-demo.html. No network needed to open it.
import { readFileSync, writeFileSync } from "node:fs";
import { join } from "node:path";

const dir = "demo-dist";
let html = readFileSync(join(dir, "demo.html"), "utf8");
html = html.replace(/<script type="module" crossorigin src="\/?([^"]+)"><\/script>/g, (_, src) => {
  const js = readFileSync(join(dir, src), "utf8").replace(/<\/script/g, "<\\/script");
  return `<script type="module">${js}</script>`;
});
html = html.replace(/<link rel="stylesheet" crossorigin href="\/?([^"]+)">/g, (_, href) => `<style>${readFileSync(join(dir, href), "utf8")}</style>`);
if (/src="\/assets|href="\/assets/.test(html)) throw new Error("something was not inlined");
writeFileSync(join(dir, "dynamis-power-demo.html"), html);
console.log(`demo-dist/dynamis-power-demo.html: ${(html.length / 1024).toFixed(0)} KB`);

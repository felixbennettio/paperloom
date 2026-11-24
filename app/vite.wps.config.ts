import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { addinMessages } from "./addin-messages";

const { version } = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as { version: string };

export default defineConfig({
  base: "./",
  publicDir: "addin-public",
  plugins: [react(), addinMessages(), {
    name: "wps-package",
    generateBundle() {
      for (const name of ["ribbon.xml", "index.html", "main.js", "pane.html"]) {
        this.emitFile({ type: "asset", fileName: name,
          source: readFileSync(new URL(`../integrations/wps/${name}`, import.meta.url)) });
      }
    },
  }],
  define: {
    __APP_VERSION__: JSON.stringify(version),
    "process.env.NODE_ENV": JSON.stringify("production"),
  },
  build: {
    outDir: "dist-wps",
    target: "chrome104",
    cssCodeSplit: false,
    lib: {
      entry: fileURLToPath(new URL("./src/addin/main.tsx", import.meta.url)),
      name: "PaperloomWpsPane",
      formats: ["iife"],
      fileName: () => "pane.js",
      cssFileName: "pane",
    },
    rollupOptions: { output: { inlineDynamicImports: true } },
  },
});

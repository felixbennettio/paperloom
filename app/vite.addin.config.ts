import { readFileSync } from "node:fs";
import { fileURLToPath } from "node:url";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vite";
import { addinMessages } from "./addin-messages";

const { version } = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as { version: string };

export default defineConfig({
  base: "./",
  publicDir: "addin-public",
  plugins: [react(), addinMessages()],
  define: { __APP_VERSION__: JSON.stringify(version) },
  resolve: {
    alias: [{ find: /^pdfjs-dist$/, replacement: "pdfjs-dist/legacy/build/pdf.mjs" }],
  },
  build: {
    outDir: "dist-addin",
    target: ["es2022", "chrome105", "safari16"],
    rollupOptions: { input: fileURLToPath(new URL("./addin.html", import.meta.url)) },
  },
});

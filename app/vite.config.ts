import { readFileSync } from "node:fs";
import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const { version } = JSON.parse(readFileSync(new URL("./package.json", import.meta.url), "utf8")) as { version: string };

// Tauri sets TAURI_DEV_HOST when developing on a phone.
const host = process.env.TAURI_DEV_HOST;

export default defineConfig({
  plugins: [react(), {
    name: "translation-runtime",
    configureServer(server) {
      server.middlewares.use((request, response, next) => {
        const name = request.url?.split("?")[0]?.slice(1);
        if (name !== "translate.html" && name !== "translate.js") { next(); return; }
        response.setHeader("Content-Type", name.endsWith(".html") ? "text/html" : "text/javascript");
        response.end(readFileSync(new URL(`../core/plugin-runtime/${name}`, import.meta.url)));
      });
    },
    generateBundle() {
      for (const name of ["translate.html", "translate.js"]) {
        this.emitFile({ type: "asset", fileName: name, source: readFileSync(new URL(`../core/plugin-runtime/${name}`, import.meta.url)) });
      }
    },
  }],
  define: { __APP_VERSION__: JSON.stringify(version) },
  resolve: {
    // pdf.js's legacy build: its modern one needs Promise.withResolvers
    // (Chrome 119, Safari 17.4), which older Android System WebViews and
    // WebKitGTK lack, and then no PDF opens there at all.
    alias: [{ find: /^pdfjs-dist$/, replacement: "pdfjs-dist/legacy/build/pdf.mjs" }],
  },
  clearScreen: false,
  server: {
    port: 1420,
    strictPort: true,
    host: host || false,
    hmr: host ? { protocol: "ws", host, port: 1421 } : undefined,
    watch: { ignored: ["**/src-tauri/**"] },
  },
  envPrefix: ["VITE_", "TAURI_ENV_*"],
  build: {
    // WebView2 on Windows, WebKitGTK on Linux, Android System WebView.
    target: ["es2022", "chrome105", "safari16"],
    minify: process.env.TAURI_ENV_DEBUG ? false : "esbuild",
    sourcemap: !!process.env.TAURI_ENV_DEBUG,
  },
  test: {
    environment: "node",
    include: ["src/**/*.test.ts"],
  },
});

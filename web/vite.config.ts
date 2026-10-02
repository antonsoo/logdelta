import { defineConfig } from "vite";
import { contentSecurityPolicy } from "./vite.csp";

export default defineConfig({
  base: "/logdelta/",
  plugins: [contentSecurityPolicy({ "script-src": ["'wasm-unsafe-eval'"] })],
  worker: { format: "es" },
  build: { target: "es2022" },
});

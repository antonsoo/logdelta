import { defineConfig } from "vite";

export default defineConfig({
  base: "/logdelta/",
  worker: { format: "es" },
  build: { target: "es2022" },
});

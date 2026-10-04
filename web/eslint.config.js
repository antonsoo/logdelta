// @ts-check
import js from "@eslint/js";
import tseslint from "typescript-eslint";

export default tseslint.config(
  { ignores: ["dist/**", "node_modules/**", "public/**", "wasm/target/**", "test-results/**", "playwright-report/**"] },
  js.configs.recommended,
  ...tseslint.configs.recommended,
  {
    files: ["scripts/**/*.mjs"],
    languageOptions: { globals: { console: "readonly", process: "readonly", URL: "readonly", fetch: "readonly", AbortSignal: "readonly", WebAssembly: "readonly" } },
  },
  {
    files: ["tests/browser/**/*.mjs"],
    languageOptions: { globals: Object.fromEntries(["window", "document", "File", "ReadableStream", "Worker", "WebAssembly", "fetch", "crypto", "URL", "localStorage", "sessionStorage", "location", "innerWidth"].map((name) => [name, "readonly"])) },
  },
);

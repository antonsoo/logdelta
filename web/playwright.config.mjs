import { defineConfig } from "@playwright/test";

export default defineConfig({
  testDir: "./tests/browser",
  fullyParallel: true,
  workers: 2,
  reporter: "list",
  use: { baseURL: "http://127.0.0.1:4193/logdelta/", trace: "retain-on-failure" },
  projects: [
    { name: "chromium", use: { browserName: "chromium" } },
    { name: "firefox", use: { browserName: "firefox" } },
  ],
  webServer: {
    command: "npm run preview -- --host 127.0.0.1 --port 4193 --strictPort",
    wait: { stdout: /Local:\s+http:\/\/127\.0\.0\.1:4193\/logdelta\// },
  },
});

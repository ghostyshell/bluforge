import { defineConfig, devices } from "@playwright/test";

/**
 * The UI is a Vite/React app; the Tauri bridge is mocked with `?mock=1`, so
 * these checks run in a plain browser on every platform (including Linux CI).
 * The native window, drive and burn paths still need a real desktop pass.
 */
export default defineConfig({
  testDir: "./tests",
  timeout: 30_000,
  expect: { timeout: 7_000 },
  fullyParallel: false,
  reporter: [["list"]],
  use: {
    baseURL: "http://127.0.0.1:4173",
    trace: "retain-on-failure",
    screenshot: "only-on-failure",
  },
  projects: [{ name: "chromium", use: { ...devices["Desktop Chrome"] } }],
  webServer: {
    command: "npm run dev -- --host 127.0.0.1 --port 4173 --strictPort",
    url: "http://127.0.0.1:4173/?mock=1",
    reuseExistingServer: !process.env.CI,
    timeout: 120_000,
  },
});

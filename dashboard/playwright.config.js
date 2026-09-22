import { defineConfig } from '@playwright/test'
import fs from 'node:fs'
import path from 'node:path'
import os from 'node:os'

// A fresh scratch dir per Playwright run: HIVEMIND_DB_PATH gives the backend
// an empty SQLite db, XDG_CONFIG_HOME keeps it from picking up whatever
// global config.toml the person running this happens to have (unrelated
// agent/hive/matrix settings that could make the server behave differently
// than a clean install). Computed once here (this file runs once per
// `playwright test` invocation), not per test -- individual tests reset
// data via the DELETE /api/v1/memories/all endpoint instead of a restart.
const scratchDir = fs.mkdtempSync(path.join(os.tmpdir(), 'hivemind-e2e-'))
const dbPath = path.join(scratchDir, 'test.db')

export default defineConfig({
  testDir: './e2e',
  fullyParallel: false,
  workers: 1,
  retries: process.env.CI ? 1 : 0,
  reporter: process.env.CI ? 'list' : 'html',
  use: {
    // 'localhost', not '127.0.0.1': vite's dev server (no explicit
    // `server.host` set) only binds the IPv6 loopback by default, which
    // 'localhost' resolves to first but '127.0.0.1' can't reach.
    baseURL: 'http://localhost:5173',
    trace: 'retain-on-failure',
  },
  webServer: [
    {
      command: 'cargo run --quiet --bin hivemind -- up --headless --plain',
      cwd: '..',
      env: {
        HIVEMIND_DB_PATH: dbPath,
        XDG_CONFIG_HOME: scratchDir,
      },
      url: 'http://127.0.0.1:3456/api/v1/status',
      timeout: 180_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
    {
      command: 'bun run dev',
      url: 'http://localhost:5173',
      timeout: 60_000,
      reuseExistingServer: false,
      stdout: 'pipe',
      stderr: 'pipe',
    },
  ],
})

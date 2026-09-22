# Dashboard test suites. Unit tests (Vitest) run in Node, no server needed.
# E2E tests (Playwright) spin up a real `hivemind up --headless` backend
# (fresh temp DB + XDG_CONFIG_HOME per run, see dashboard/playwright.config.js)
# plus `vite dev`, then drive a real browser against them.

[working-directory: 'dashboard']
e2e-unit:
  bun run test:unit

# Headless, CI-friendly: no visible browser window.
[working-directory: 'dashboard']
e2e-headless:
  bun run test:e2e

# Headed + single worker, for watching the browser interact locally.
[working-directory: 'dashboard']
e2e-browser:
  bun run test:e2e:ui

# Both suites, headless -- what CI should run.
e2e: e2e-unit e2e-headless

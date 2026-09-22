import { test as base, expect } from '@playwright/test'

const API_BASE = 'http://127.0.0.1:3456'

// Every test gets a clean memory store (DELETE /api/v1/memories/all) --
// the backend itself is only spun up once per Playwright run (see
// playwright.config.js's webServer), so tests can't rely on process
// isolation the way the Rust test suite does.
export const test = base.extend({
  api: async ({ request }, use) => {
    await request.delete(`${API_BASE}/api/v1/memories/all`)
    await use(request)
  },
})

export { expect, API_BASE }

export async function createMemory(request, { title, content = 'e2e test content', tags = [] } = {}) {
  const resp = await request.post(`${API_BASE}/api/v1/memories`, {
    data: { title, content, tags },
  })
  expect(resp.ok()).toBeTruthy()
  return resp.json()
}

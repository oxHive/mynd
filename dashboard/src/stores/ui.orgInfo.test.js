import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

vi.mock('../api/memories.js', () => ({
  getStatus: vi.fn(),
}))

import { getStatus } from '../api/memories.js'
import { useUiStore } from './ui.js'

describe('ui store orgInfo', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('populates orgInfo from the status response org block', async () => {
    getStatus.mockResolvedValue({
      sync: { enabled: false },
      org: { configured: true, enabled: true, last_synced_at: 123, conflict_count: 0 },
    })
    const ui = useUiStore()
    await ui.pollServerStatus()
    expect(ui.orgInfo).toEqual({ configured: true, enabled: true, last_synced_at: 123, conflict_count: 0 })
  })

  it('sets orgInfo to null when the status response has no org block', async () => {
    getStatus.mockResolvedValue({ sync: { enabled: false } })
    const ui = useUiStore()
    await ui.pollServerStatus()
    expect(ui.orgInfo).toBeNull()
  })
})

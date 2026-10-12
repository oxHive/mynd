import { describe, it, expect, vi, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'

vi.mock('../api/settings.js', () => ({
  getConfigFile: vi.fn(),
  saveConfigFile: vi.fn(),
  restartServer: vi.fn(),
}))

import { getConfigFile } from '../api/settings.js'
import { useConfigFileStore } from './configFile.js'

const response = (over = {}) => ({
  values: { server: { port: 4000 }, sync: { interval_seconds: 60 } },
  api_keys: { sync: 'file', org_sync: 'none' },
  editable: true, path: '/x/config.toml', restart_required: false, ...over,
})

describe('configFile store', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
    vi.clearAllMocks()
  })

  it('patches only changed keys, with null for removed ones and api key drafts', async () => {
    getConfigFile.mockResolvedValue(response())
    const config = useConfigFileStore()
    await config.fetch()
    expect(config.isDirty).toBe(false)
    config.set('server', 'host', '0.0.0.0')
    config.set('sync', 'interval_seconds', '')
    config.apiKeyDrafts.org_sync = 'k'
    expect(config.isDirty).toBe(true)
    expect(config.patch()).toEqual({
      server: { host: '0.0.0.0' },
      sync: { interval_seconds: null },
      org_sync: { api_key: 'k' },
    })
    config.discard()
    expect(config.isDirty).toBe(false)
  })

  it('reports locked when the file sets allow_config_edit = false', async () => {
    getConfigFile.mockResolvedValue(response({ editable: false }))
    const config = useConfigFileStore()
    await config.fetch()
    expect(config.locked).toBe(true)
  })
})

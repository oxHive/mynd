import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import AppSidebar from './AppSidebar.vue'
import { useUiStore } from '../../stores/ui.js'

describe('AppSidebar org status row', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('shows an org status row when org is configured', () => {
    const ui = useUiStore()
    ui.orgInfo = { configured: true, enabled: true, last_synced_at: null, conflict_count: 0 }
    const wrapper = mount(AppSidebar)
    expect(wrapper.text()).toContain('org')
  })

  it('does not show an org status row when org is not configured', () => {
    const ui = useUiStore()
    ui.orgInfo = null
    const wrapper = mount(AppSidebar)
    expect(wrapper.text()).not.toContain('org')
  })
})

describe('AppSidebar connection status', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('stays connected when only sync failed', () => {
    const ui = useUiStore()
    ui.serverStatus = 'sync_failed'
    const wrapper = mount(AppSidebar)
    expect(wrapper.text()).toContain('Connected')
    expect(wrapper.text()).not.toContain('Disconnected')
    expect(wrapper.text()).toContain('sync failed')
  })

  it('shows disconnected when the server is unreachable', () => {
    const ui = useUiStore()
    ui.serverStatus = 'unreachable'
    expect(mount(AppSidebar).text()).toContain('Disconnected')
  })

  it('warns when the last sync is older than 10 minutes', () => {
    const ui = useUiStore()
    ui.serverStatus = 'running'
    ui.syncInfo = { enabled: true, last_synced_at: Math.floor(Date.now() / 1000) - 20 * 60 }
    const label = mount(AppSidebar).find('.text-gold-dark')
    expect(label.exists()).toBe(true)
    expect(label.text()).toBe('synced 20m ago')
  })
})

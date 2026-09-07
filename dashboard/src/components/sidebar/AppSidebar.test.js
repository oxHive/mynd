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

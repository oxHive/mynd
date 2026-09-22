import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import GraphToolbar from './GraphToolbar.vue'
import { useUiStore } from '../../stores/ui.js'

describe('GraphToolbar', () => {
  it('includes an Org layer tab when the org layer is configured', () => {
    setActivePinia(createPinia())
    useUiStore().orgInfo = { configured: true }
    const wrapper = mount(GraphToolbar, {
      global: { stubs: { TagFilter: true } },
    })
    expect(wrapper.text()).toContain('Org')
  })

  it('omits the Org layer tab when the org layer is not configured', () => {
    setActivePinia(createPinia())
    const wrapper = mount(GraphToolbar, {
      global: { stubs: { TagFilter: true } },
    })
    expect(wrapper.text()).not.toContain('Org')
  })
})

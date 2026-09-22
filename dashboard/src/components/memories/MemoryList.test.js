import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import MemoryList from './MemoryList.vue'
import { useUiStore } from '../../stores/ui.js'

describe('MemoryList', () => {
  it('includes an org filter chip when the org layer is configured', () => {
    setActivePinia(createPinia())
    useUiStore().orgInfo = { configured: true }
    const wrapper = mount(MemoryList, {
      global: { stubs: { TagFilter: true, MemoryCard: true, SkeletonCard: true } },
    })
    expect(wrapper.text()).toContain('org')
  })

  it('omits the org filter chip when the org layer is not configured', () => {
    setActivePinia(createPinia())
    const wrapper = mount(MemoryList, {
      global: { stubs: { TagFilter: true, MemoryCard: true, SkeletonCard: true } },
    })
    expect(wrapper.text()).not.toContain('org')
  })
})

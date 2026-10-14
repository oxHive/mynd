import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import MemoryList from './MemoryList.vue'
import { useUiStore } from '../../stores/ui.js'
import MemoryTree from './MemoryTree.vue'
import { useMemoriesStore } from '../../stores/memories.js'

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

  it('shows the tree for the alphabetically first project when the tree chip is picked', async () => {
    setActivePinia(createPinia())
    useMemoriesStore().all = [
      { id: 'm1', title: 't1', content: 'c', tags: ['project:zeta'] },
      { id: 'm2', title: 't2', content: 'c', tags: ['project:alpha'] },
    ]
    const wrapper = mount(MemoryList, {
      global: { stubs: { TagFilter: true, MemoryCard: true, SkeletonCard: true, MemoryTree: true } },
    })
    await wrapper.findAll('button').find(b => b.text() === 'tree').trigger('click')
    const tree = wrapper.findComponent(MemoryTree)
    expect(tree.exists()).toBe(true)
    expect(tree.props('project')).toBe('alpha')
  })
})

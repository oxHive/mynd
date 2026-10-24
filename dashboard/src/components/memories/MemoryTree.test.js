import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'

vi.mock('../../api/memories.js', () => ({
  getProjectTree: vi.fn(async () => ({
    project: 'mynd',
    layer: 'workspace',
    root_id: 'r',
    nodes: [
      { id: 'r', title: 'mynd root', role: 'root', tokens: 9, homes: [], updated_at: 0, reachable: true,
        pointers: [{ target_id: 'l', text: 'Leaf: what it holds; open when needed' }] },
      { id: 'l', title: 'leaf', role: null, tokens: 3, homes: ['r'], updated_at: 0, reachable: true, pointers: [] },
      { id: 'o', title: 'orphan', role: null, tokens: 3, homes: [], updated_at: 0, reachable: false, pointers: [] },
    ],
  })),
}))

import MemoryTree from './MemoryTree.vue'
import { useMemoriesStore } from '../../stores/memories.js'

describe('MemoryTree', () => {
  it('renders the outline with pointer text and an unhomed section, and opens a memory on click', async () => {
    setActivePinia(createPinia())
    const memories = useMemoriesStore()
    memories.all = [{ id: 'l', title: 'leaf', content: 'c', tags: [] }]
    const select = vi.spyOn(memories, 'select')

    const wrapper = mount(MemoryTree, { props: { project: 'mynd' } })
    await flushPromises()

    expect(wrapper.text()).toContain('mynd root')
    expect(wrapper.text()).toContain('Leaf: what it holds; open when needed')
    expect(wrapper.text()).toContain('Unhomed')
    expect(wrapper.text()).toContain('orphan')

    await wrapper.findAll('button').find(b => b.text().startsWith('leaf')).trigger('click')
    expect(select).toHaveBeenCalledWith(memories.all[0])
  })
})

import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import MemoryDetail from './MemoryDetail.vue'
import { useMemoriesStore } from '../../stores/memories.js'
import { useUiStore } from '../../stores/ui.js'

describe('MemoryDetail org layer picker gating', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  function mountDetail() {
    return mount(MemoryDetail, {
      global: {
        stubs: {
          TagInput: true, MarkdownContent: true, DeleteConfirmModal: true,
          ConfirmModal: true, CopyIdButton: true, EmptyState: true,
        },
      },
    })
  }

  it('disables the org layer button when org is not configured', () => {
    const memories = useMemoriesStore()
    const ui = useUiStore()
    ui.orgInfo = null
    memories.startNew()
    const wrapper = mountDetail()
    const orgBtn = wrapper.findAll('button').find(b => b.text() === 'org')
    expect(orgBtn.attributes('disabled')).toBeDefined()
  })

  it('enables the org layer button when org is configured', () => {
    const memories = useMemoriesStore()
    const ui = useUiStore()
    ui.orgInfo = { configured: true, enabled: true, last_synced_at: null, conflict_count: 0, count: 0 }
    memories.startNew()
    const wrapper = mountDetail()
    const orgBtn = wrapper.findAll('button').find(b => b.text() === 'org')
    expect(orgBtn.attributes('disabled')).toBeUndefined()
  })
})

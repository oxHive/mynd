import { describe, it, expect, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useMemoriesStore } from './memories.js'

describe('memories store layerFilter', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('filters to only org-layer entries when layerFilter is "org"', () => {
    const memories = useMemoriesStore()
    memories.all = [
      { id: '1', title: 'a', content: '', tags: [], layer: 'workspace' },
      { id: '2', title: 'b', content: '', tags: [], layer: 'org' },
    ]
    memories.layerFilter = 'org'
    expect(memories.filtered.map((m) => m.id)).toEqual(['2'])
  })
})

import { describe, it, expect, beforeEach } from 'vitest'
import { setActivePinia, createPinia } from 'pinia'
import { useGraphStore } from './graph.js'

describe('graph store layerFilter', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('accepts "org" as a layerFilter value', () => {
    const graph = useGraphStore()
    graph.layerFilter = 'org'
    expect(graph.layerFilter).toBe('org')
  })
})

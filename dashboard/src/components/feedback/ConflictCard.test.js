import { describe, it, expect, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'
import ConflictCard from './ConflictCard.vue'

describe('ConflictCard layer badge', () => {
  beforeEach(() => {
    setActivePinia(createPinia())
  })

  it('shows an org badge when the conflict has layer "org"', () => {
    const wrapper = mount(ConflictCard, {
      props: {
        conflict: { id: 'cfl_1', title: 'Conflict', local: 'a', current: 'b', layer: 'org' },
      },
    })
    expect(wrapper.text()).toContain('org')
  })

  it('shows no badge when the conflict has no layer field', () => {
    const wrapper = mount(ConflictCard, {
      props: {
        conflict: { id: 'cfl_2', title: 'Conflict', local: 'a', current: 'b' },
      },
    })
    expect(wrapper.text()).not.toContain('org')
  })
})

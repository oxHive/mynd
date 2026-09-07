import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import LayerBadge from './LayerBadge.vue'

describe('LayerBadge', () => {
  it('renders org for layer="org"', () => {
    const wrapper = mount(LayerBadge, { props: { layer: 'org' } })
    expect(wrapper.text()).toBe('org')
  })

  it('still renders personal for layer="personal"', () => {
    const wrapper = mount(LayerBadge, { props: { layer: 'personal' } })
    expect(wrapper.text()).toBe('personal')
  })

  it('still defaults to workspace for any other value', () => {
    const wrapper = mount(LayerBadge, { props: { layer: 'workspace' } })
    expect(wrapper.text()).toBe('workspace')
  })
})

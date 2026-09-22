import { describe, it, expect } from 'vitest'
import { mount } from '@vue/test-utils'
import FilterChip from './FilterChip.vue'

describe('FilterChip', () => {
  it('applies org styling when active and layer="org"', () => {
    const wrapper = mount(FilterChip, {
      props: { label: 'org', value: 'org', active: true, layer: 'org' },
    })
    expect(wrapper.attributes('style')).toContain('--hm-org')
  })

  it('does not apply org styling when inactive', () => {
    const wrapper = mount(FilterChip, {
      props: { label: 'org', value: 'org', active: false, layer: 'org' },
    })
    expect(wrapper.attributes('style')).not.toContain('--hm-org')
  })
})

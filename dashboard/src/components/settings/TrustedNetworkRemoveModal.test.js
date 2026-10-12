import { describe, it, expect, vi, beforeEach } from 'vitest'
import { mount } from '@vue/test-utils'
import { setActivePinia, createPinia } from 'pinia'

vi.mock('../../api/hive.js', () => ({ removeTrustedNetwork: vi.fn() }))

import TrustedNetworkRemoveModal from './TrustedNetworkRemoveModal.vue'

describe('TrustedNetworkRemoveModal', () => {
  beforeEach(() => setActivePinia(createPinia()))

  it('enables Remove only once the bare SSID is typed, and blocks paste', async () => {
    const w = mount(TrustedNetworkRemoveModal, {
      props: { network: { id: 'ssid:home-wifi', label: null } },
      attachTo: document.body,
    })
    const input = document.querySelector('#confirm-network')
    const remove = [...document.querySelectorAll('button')].find(b => b.textContent.trim() === 'Remove')
    expect(remove.disabled).toBe(true)

    const paste = new Event('paste', { cancelable: true })
    input.dispatchEvent(paste)
    expect(paste.defaultPrevented).toBe(true)

    input.value = 'ssid:home-wifi'
    input.dispatchEvent(new Event('input'))
    await w.vm.$nextTick()
    expect(remove.disabled).toBe(true)

    input.value = 'home-wifi'
    input.dispatchEvent(new Event('input'))
    await w.vm.$nextTick()
    expect(remove.disabled).toBe(false)
    w.unmount()
  })
})

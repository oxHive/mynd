import { describe, it, expect, vi } from 'vitest'
import { mount, flushPromises } from '@vue/test-utils'
import { createPinia, setActivePinia } from 'pinia'

vi.mock('../../api/audit.js', () => ({
  startSession: vi.fn(async () => ({})),
  getSession: vi.fn(async () => ({ active: false, phase: 'idle' })),
  reviseSession: vi.fn(async () => ({})),
  endSession: vi.fn(async () => ({})),
  listProposals: vi.fn(async () => ({ proposals: [] })),
  applyProposal: vi.fn(async () => ({ applied: true })),
  rejectProposal: vi.fn(async () => ({ rejected: true })),
}))

import * as api from '../../api/audit.js'
import AuditPanel from './AuditPanel.vue'
import { useAuditStore } from '../../stores/audit.js'
import { useMemoriesStore } from '../../stores/memories.js'

const proposal = {
  id: 'prop_1', project: 'mynd', layer: 'workspace', kind: 'stale-pointer',
  summary: 'Refresh the libsql pointer', reason: 'the interval changed', status: 'pending',
  base: {}, created_at: 0,
  ops: [{ op: 'update', id: 'm1', content: '# Sync\n- [libsql: 60s interval](child:x)' }],
}

describe('AuditPanel', () => {
  it('shows a proposal with its diff and applies it on approve', async () => {
    setActivePinia(createPinia())
    useMemoriesStore().all = [{ id: 'm1', title: 'Sync index', content: '# Sync\n- [libsql: 300s interval](child:x)', tags: ['project:mynd'] }]
    const audit = useAuditStore()
    audit.proposals = [proposal]
    audit.project = 'mynd'

    const wrapper = mount(AuditPanel)
    expect(wrapper.text()).toContain('Refresh the libsql pointer')
    expect(wrapper.text()).toContain('stale-pointer')
    await wrapper.findAll('button').find(b => b.text() === 'Details').trigger('click')
    expect(wrapper.text()).toContain('- [libsql: 300s interval](child:x)')
    expect(wrapper.text()).toContain('- [libsql: 60s interval](child:x)')

    await wrapper.findAll('button').find(b => b.text() === 'Approve').trigger('click')
    await flushPromises()
    expect(api.applyProposal).toHaveBeenCalledWith('prop_1')
  })

  it('shows the stale message when apply returns 409', async () => {
    setActivePinia(createPinia())
    api.applyProposal.mockRejectedValueOnce(Object.assign(new Error('409 Conflict'), { status: 409, detail: 'stale, re-run the audit: m1 changed' }))
    const audit = useAuditStore()
    audit.proposals = [proposal]
    audit.project = 'mynd'
    const wrapper = mount(AuditPanel)
    await wrapper.findAll('button').find(b => b.text() === 'Approve').trigger('click')
    await flushPromises()
    expect(wrapper.text()).toContain('stale, re-run the audit')
  })
})

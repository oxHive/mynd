import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '../api/audit.js'

export const useAuditStore = defineStore('audit', () => {
  const active = ref(false)
  const phase = ref('idle') // 'idle' | 'auditing' | 'reviewing' | 'revising'
  const project = ref('')
  const proposals = ref([]) // every pending proposal, all projects
  const revisingProposalId = ref(null)
  const queuedProposalIds = ref([])
  const error = ref(null)
  const panelOpen = ref(false)
  const auditingStartedAt = ref(null) // ms epoch; null when not auditing

  async function fetchProposals() {
    try {
      const res = await api.listProposals()
      proposals.value = res.proposals ?? []
    } catch { /* server without the feature; keep what we have */ }
  }

  async function start(name) {
    error.value = null
    project.value = name
    try {
      // The 'started' SSE event is the source of truth for active/phase: a
      // fast-failing agent can emit 'started' and 'error' before this resolves.
      await api.startSession(name)
      panelOpen.value = true
    } catch (e) {
      error.value = e.status === 409 ? 'An audit is already running.' : (e.detail || String(e.message))
      panelOpen.value = true
    }
  }

  async function revise(id, feedback) {
    error.value = null
    try {
      await api.reviseSession(id, feedback)
      if (!queuedProposalIds.value.includes(id)) queuedProposalIds.value.push(id)
    } catch (e) {
      error.value = e.detail || String(e.message)
    }
  }

  async function apply(id) {
    error.value = null
    try {
      await api.applyProposal(id)
      proposals.value = proposals.value.filter(p => p.id !== id)
    } catch (e) {
      error.value = e.detail || String(e.message)
    }
  }

  async function reject(id) {
    error.value = null
    try {
      await api.rejectProposal(id)
      proposals.value = proposals.value.filter(p => p.id !== id)
    } catch (e) {
      error.value = e.detail || String(e.message)
    }
  }

  async function end() {
    try { await api.endSession() } catch { /* state resets via SSE or below */ }
    active.value = false
    phase.value = 'idle'
    revisingProposalId.value = null
    queuedProposalIds.value = []
    auditingStartedAt.value = null
  }

  async function hydrate() {
    try {
      const s = await api.getSession()
      active.value = !!s.active
      phase.value = s.phase ?? 'idle'
      if (s.project) project.value = s.project
      revisingProposalId.value = s.revising_proposal_id ?? null
      queuedProposalIds.value = s.queued_proposal_ids ?? []
      auditingStartedAt.value = phase.value === 'auditing' ? Date.now() : null
      if (active.value) panelOpen.value = true
    } catch { /* server without the feature; leave idle */ }
    await fetchProposals()
  }

  function handleEvent(data) {
    switch (data.state) {
      case 'started':
        active.value = true; phase.value = 'auditing'; panelOpen.value = true
        if (data.project) project.value = data.project
        auditingStartedAt.value = Date.now()
        break
      case 'proposals_ready':
        phase.value = 'reviewing'; auditingStartedAt.value = null
        fetchProposals()
        break
      case 'revising':
        phase.value = 'revising'
        revisingProposalId.value = data.proposal_id ?? null
        queuedProposalIds.value = data.queued ?? []
        break
      case 'revision_ready':
        phase.value = 'reviewing'; revisingProposalId.value = null
        fetchProposals()
        break
      case 'error':
        phase.value = active.value ? 'reviewing' : 'idle'
        revisingProposalId.value = null
        error.value = data.message ?? 'agent error'
        auditingStartedAt.value = null
        break
      case 'ended':
        active.value = false; phase.value = 'idle'
        revisingProposalId.value = null; queuedProposalIds.value = []
        auditingStartedAt.value = null
        break
    }
  }

  function openPanel() { panelOpen.value = true }
  function closePanel() { panelOpen.value = false }

  return {
    active, phase, project, proposals, revisingProposalId, queuedProposalIds, error, panelOpen,
    auditingStartedAt, fetchProposals, start, revise, apply, reject, end, hydrate, handleEvent,
    openPanel, closePanel,
  }
})

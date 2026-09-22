import { defineStore } from 'pinia'
import { ref } from 'vue'
import * as api from '../api/suggest.js'

export const useSuggestStore = defineStore('suggest', () => {
  const active = ref(false)
  const phase = ref('idle') // 'idle' | 'suggesting' | 'reviewing' | 'revising'
  const revisingEdgeId = ref(null)
  const queuedEdgeIds = ref([])
  const error = ref(null)
  const panelOpen = ref(false)
  const suggestingStartedAt = ref(null) // ms epoch; null when not analyzing

  async function start() {
    error.value = null
    try {
      await api.startSession()
      // Don't set active/phase/suggestingStartedAt here: for a fast-failing
      // agent (e.g. a bad `[agent] command`), the worker can emit its
      // 'started' *and* 'error' SSE events before this request even
      // resolves, and setting them here would clobber that already-applied
      // 'error' state back to 'suggesting' with no further event to correct
      // it. The 'started' SSE event (handleEvent below) is the sole source
      // of truth for those fields.
      panelOpen.value = true
    } catch (e) {
      error.value = e.status === 409 ? 'A suggest session is already running.' : String(e.message)
      panelOpen.value = true
    }
  }

  async function revise(edgeId, feedback) {
    error.value = null
    try {
      await api.reviseSession(edgeId, feedback)
      if (!queuedEdgeIds.value.includes(edgeId)) queuedEdgeIds.value.push(edgeId)
    } catch (e) {
      error.value = String(e.message)
    }
  }

  async function end() {
    try { await api.endSession() } catch { /* session state resets via SSE or below */ }
    active.value = false
    phase.value = 'idle'
    revisingEdgeId.value = null
    queuedEdgeIds.value = []
    suggestingStartedAt.value = null
  }

  async function hydrate() {
    try {
      const s = await api.getSession()
      active.value = !!s.active
      phase.value = s.phase ?? 'idle'
      revisingEdgeId.value = s.revising_edge_id ?? null
      queuedEdgeIds.value = s.queued_edge_ids ?? []
      // No server-side start timestamp to hydrate exactly; approximate from now
      // so a page reload mid-analysis still shows a (slightly short) timer.
      suggestingStartedAt.value = phase.value === 'suggesting' ? Date.now() : null
      if (active.value) panelOpen.value = true
    } catch { /* server without the feature; leave idle */ }
  }

  function handleEvent(data) {
    switch (data.state) {
      case 'started':
        active.value = true; phase.value = 'suggesting'; panelOpen.value = true
        suggestingStartedAt.value = Date.now()
        break
      case 'suggestions_ready':
        phase.value = 'reviewing'; suggestingStartedAt.value = null; break
      case 'revising':
        phase.value = 'revising'
        revisingEdgeId.value = data.edge_id ?? null
        queuedEdgeIds.value = data.queued ?? []
        break
      case 'revision_ready':
        phase.value = 'reviewing'; revisingEdgeId.value = null; break
      case 'error':
        phase.value = active.value ? 'reviewing' : 'idle'
        revisingEdgeId.value = null
        error.value = data.message ?? 'agent error'
        suggestingStartedAt.value = null
        break
      case 'ended':
        active.value = false; phase.value = 'idle'
        revisingEdgeId.value = null; queuedEdgeIds.value = []
        suggestingStartedAt.value = null
        break
    }
  }

  function openPanel() { panelOpen.value = true }
  function closePanel() { panelOpen.value = false }

  return {
    active, phase, revisingEdgeId, queuedEdgeIds, error, panelOpen, suggestingStartedAt,
    start, revise, end, hydrate, handleEvent, openPanel, closePanel,
  }
})

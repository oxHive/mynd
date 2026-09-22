import { defineStore } from 'pinia'
import { ref, computed } from 'vue'
import * as api from '../api/edges.js'
import { useMemoriesStore } from './memories.js'
import { withMention } from '../lib/mention.js'

export const useGraphStore = defineStore('graph', () => {
  const edges = ref([])
  const zoom = ref(2)            // 1|2|3
  const selectedNodeId = ref(null)
  const selectedEdgeId = ref(null)
  const searchQuery = ref('')
  const layerFilter = ref('all') // 'all' | 'personal' | 'workspace'
  // '' (off), 'ns:*' (any value in namespace ns), 'ns:value', or a bare tag.
  // Set by TagFilter.vue, read by GraphCanvas.vue's node-dimming pass.
  const tagFilter = ref('')
  // Bumped by the toolbar's re-layout button; GraphCanvas watches it and
  // repacks the layout from scratch (clearing user-pinned positions).
  const relayoutSeq = ref(0)

  function requestRelayout() {
    relayoutSeq.value++
  }

  function matchesTagFilter(tags) {
    if (!tagFilter.value) return true
    const list = (tags || []).map(t => t.toLowerCase())
    if (tagFilter.value.endsWith(':*')) {
      const prefix = tagFilter.value.slice(0, -1)
      return list.some(t => t.startsWith(prefix))
    }
    return list.includes(tagFilter.value)
  }

  const pendingEdges = computed(() => edges.value.filter(e => e.status === 'pending'))
  const selectedEdge = computed(() => edges.value.find(e => e.id === selectedEdgeId.value) || null)

  function edgesFor(memoryId) {
    return edges.value.filter(e =>
      (e.source_id === memoryId || e.target_id === memoryId) &&
      e.status === 'active'
    )
  }

  async function fetchEdges() {
    const data = await api.listEdges()
    edges.value = data.edges ?? []
  }

  // Approving a suggested (pending) connection also embeds it as a mention
  // link in the source memory's content — the dashboard is what applies the
  // edit the agent only proposed, keeping the review step meaningful.
  async function resolveEdge(id, status) {
    const before = edges.value.find(e => e.id === id)
    await api.patchEdge(id, { status })
    const idx = edges.value.findIndex(e => e.id === id)
    if (idx !== -1) edges.value[idx] = { ...edges.value[idx], status }

    if (status === 'active' && before?.status === 'pending' && before.link_text) {
      const memories = useMemoriesStore()
      const source = memories.all.find(m => m.id === before.source_id)
      if (source) {
        const nextContent = withMention(source.content, before)
        if (nextContent !== source.content) {
          await memories.patchContent(before.source_id, nextContent)
        }
      }
    }
  }

  async function acceptAllPending() {
    await Promise.all(pendingEdges.value.map(e => resolveEdge(e.id, 'active')))
  }

  async function rejectAllPending() {
    await Promise.all(pendingEdges.value.map(e => resolveEdge(e.id, 'rejected')))
  }

  return {
    edges, zoom, selectedNodeId, selectedEdgeId, searchQuery, layerFilter, tagFilter,
    relayoutSeq, requestRelayout,
    pendingEdges, selectedEdge, edgesFor, fetchEdges, resolveEdge,
    acceptAllPending, rejectAllPending, matchesTagFilter,
  }
})

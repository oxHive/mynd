// Pure derivations for the Analytics view. Ported from the Claude Design
// handoff (design_handoff_analytics/vue/useAnalytics.ts) — math kept as-is,
// field names adapted to this app's actual shapes:
//   Memory.type -> Memory.memory_type ('preference'|'project'|'history')
//   Memory.layer: 'personal'|'workspace'|'org' (handoff assumed 2 layers)
//   Edge.status: 'active'|'pending'|'rejected' (handoff used 'accepted')
//   Edge has no origin field — derived via originOf() from link_text/reason.
import { computed } from 'vue'

export const DAY = 86400 // seconds; all timestamps are unix seconds
export const LAYERS = ['personal', 'workspace', 'org']

// content-parsed [phrase](kind:mem_xxx) edges always carry link_text and
// never a reason (see src/store.rs sync_relationship_edges); edges created
// via the memory_store_edge MCP tool never carry link_text, and carry a
// reason only when the agent explains why it's proposing the link.
export function originOf(edge) {
  if (edge.link_text) return 'auto'
  if (edge.reason) return 'inferred'
  return 'manual'
}

function emptyLayerCounts() {
  return { personal: 0, workspace: 0, org: 0 }
}

export function buildBuckets(memories, rangeDays, now) {
  if (!memories.length) return { buckets: [], step: 1 }
  const earliest = memories.reduce((a, m) => Math.min(a, m.created_at), Infinity)
  const start = rangeDays ? now - rangeDays * DAY : earliest - DAY
  const spanDays = Math.max(1, Math.ceil((now - start) / DAY) + 1)
  const step = spanDays <= 34 ? 1 : spanDays <= 110 ? 7 : 30 // daily / weekly / ~monthly
  const n = Math.ceil(spanDays / step)
  const buckets = []
  for (let i = 0; i < n; i++) {
    const s = start + i * step * DAY
    const e = s + step * DAY
    let count = 0, cum = 0
    for (const m of memories) {
      if (m.created_at < e) cum++
      if (m.created_at >= s && m.created_at < e) count++
    }
    buckets.push({ s, e, count, cum })
  }
  return { buckets, step }
}

export function useAnalytics(memoriesRef, edgesRef, nowRef) {
  return computed(() => {
    const M = memoriesRef.value, E = edgesRef.value, NOW = nowRef.value
    const tagCount = new Map()
    const typeCount = new Map()
    for (const m of M) {
      for (const t of (m.tags || [])) {
        const c = tagCount.get(t) ?? emptyLayerCounts()
        c[m.layer] = (c[m.layer] ?? 0) + 1
        tagCount.set(t, c)
      }
      const tc = typeCount.get(m.memory_type) ?? emptyLayerCounts()
      tc[m.layer] = (tc[m.layer] ?? 0) + 1
      typeCount.set(m.memory_type, tc)
    }
    const withTotal = (c) => c.personal + c.workspace + c.org
    const topTags = [...tagCount.entries()]
      .map(([tag, c]) => ({ tag, ...c, total: withTotal(c) }))
      .sort((x, y) => y.total - x.total || x.tag.localeCompare(y.tag))
      .slice(0, 10)
    const types = [...typeCount.entries()]
      .map(([type, c]) => ({ type, ...c, total: withTotal(c) }))
      .sort((x, y) => y.total - x.total)

    const personal = M.filter((m) => m.layer === 'personal').length
    const workspace = M.filter((m) => m.layer === 'workspace').length
    const org = M.filter((m) => m.layer === 'org').length
    const tagRefs = M.reduce((s, m) => s + (m.tags?.length || 0), 0)

    const accepted = E.filter((e) => e.status === 'active')
    const pending = E.filter((e) => e.status === 'pending')
    const rejected = E.filter((e) => e.status === 'rejected')
    const degree = new Map()
    for (const e of accepted) {
      degree.set(e.source_id, (degree.get(e.source_id) ?? 0) + 1)
      degree.set(e.target_id, (degree.get(e.target_id) ?? 0) + 1)
    }
    const hubs = M.map((m) => ({ m, deg: degree.get(m.id) ?? 0 }))
      .sort((x, y) => y.deg - x.deg).slice(0, 5)
    const orphans = M.filter((m) => !degree.has(m.id)).length
    const byOrigin = ['manual', 'inferred', 'auto'].map((t) => ({
      t, n: accepted.filter((e) => originOf(e) === t).length,
    }))

    const in90 = M.filter((m) => m.created_at > NOW - 90 * DAY).length
    const prev90 = M.filter((m) => m.created_at > NOW - 180 * DAY && m.created_at <= NOW - 90 * DAY).length
    const touched7 = M.filter((m) => m.updated_at > NOW - 7 * DAY).length
    const staleDays = 90
    const stale = M.filter((m) => m.updated_at < NOW - staleDays * DAY).length

    const edgeTotal = Math.max(1, accepted.length + pending.length + rejected.length)
    return {
      total: M.length, personal, workspace, org,
      topTags, types, distinctTags: tagCount.size, tagRefs,
      tagsPerMemory: (tagRefs / Math.max(1, M.length)).toFixed(1),
      accepted: accepted.length, pending: pending.length, rejected: rejected.length, edgeTotal,
      avgDegree: M.length ? ((accepted.length * 2) / M.length).toFixed(1) : '0.0',
      hubs, maxDeg: Math.max(1, ...hubs.map((h) => h.deg)),
      orphans, byOrigin, in90, prev90, touched7, stale, staleDays,
      maxTag: topTags[0]?.total ?? 1, maxType: types[0]?.total ?? 1,
    }
  })
}

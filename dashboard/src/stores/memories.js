import { defineStore } from 'pinia'
import { ref, computed, watch } from 'vue'
import * as api from '../api/memories.js'
import { readStored } from '../lib/localStore.js'

const DRAFTS_KEY = 'mynd.memories.drafts'
// Sentinel key for the in-progress "new memory" draft in the same
// id-keyed stash used for existing memories' unsaved edits — there's no
// real id to key it by until it's actually saved.
const NEW_DRAFT_KEY = '__new__'

function isNewDraftFilled(d) {
  return !!(d && (d.title?.trim() || d.content?.trim() || (d.tags && d.tags.length)))
}

function loadStashedDrafts() {
  try {
    const raw = readStored(DRAFTS_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

export const useMemoriesStore = defineStore('memories', () => {
  const all = ref([])
  const selected = ref(null)
  const draft = ref(null)
  // Unsaved edits per memory id, stashed when switching away so re-selecting
  // the same memory restores them instead of silently discarding — switching
  // views (e.g. to Settings) never touches this store, so drafts survive
  // there for free; switching memories used to always wipe the draft, which
  // read as an inconsistency between the two navigation paths. Persisted to
  // localStorage (loaded here, written by persistDrafts) so a page reload
  // doesn't silently discard unsaved edits either.
  const stashedDrafts = ref(loadStashedDrafts())
  // Set when the selected memory is changed elsewhere (e.g. by an agent via
  // MCP) while a dirty draft is open here — holds the incoming server
  // version so it isn't silently lost. `selected` is frozen at the version
  // the draft was diffing against until the user resolves it, so `dirty`/
  // Save keep comparing against a baseline that still makes sense.
  const conflict = ref(null)
  const searchQuery = ref('')
  const layerFilter = ref('all')
  // '' (off), 'ns:*' (any value in namespace ns), 'ns:value', or a bare tag.
  // Set by TagFilter.vue. Applied client-side (not through the tag_query
  // grammar) so the namespace-wildcard case works without a server change.
  const tagFilter = ref('')
  const loading = ref(false)
  const saving = ref(false)
  // True while composing a brand-new memory in the detail panel — no id
  // yet, nothing sent to the backend until Save.
  const creatingNew = ref(false)

  const canSaveNew = computed(() =>
    !!(creatingNew.value && draft.value?.title?.trim() && draft.value?.content?.trim())
  )

  // Whether there's a stashed new-memory draft waiting — either the one
  // currently being composed (if it has anything in it) or one left behind
  // from switching away earlier.
  const hasNewDraft = computed(() =>
    (creatingNew.value && isNewDraftFilled(draft.value)) ||
    Object.prototype.hasOwnProperty.call(stashedDrafts.value, NEW_DRAFT_KEY)
  )

  // Mirrors src/tag_query.rs's `looks_like_tag_expr` — same three prefixes.
  // Kept as a single-line sniff, not a re-implementation of the grammar:
  // matching queries are evaluated server-side via api.searchMemories, so
  // there's exactly one place that understands `tag:a & tag:b` syntax.
  function looksLikeTagExpr(q) {
    const t = q.trim()
    return t.startsWith('tag:') || t.startsWith('!tag:') || t.startsWith('(')
  }

  // Set by the searchQuery watcher below when the query is a tag expression
  // — holds the server's evaluation of it. Null means "not a tag expression,
  // use the local substring filter instead."
  const tagExprResults = ref(null)

  watch(searchQuery, async (q) => {
    if (!looksLikeTagExpr(q)) {
      tagExprResults.value = null
      return
    }
    try {
      const res = await api.searchMemories(q)
      tagExprResults.value = res.results
    } catch {
      tagExprResults.value = []
    }
  })

  function matchesTagFilter(tags) {
    if (!tagFilter.value) return true
    const list = (tags || []).map(t => t.toLowerCase())
    if (tagFilter.value.endsWith(':*')) {
      const prefix = tagFilter.value.slice(0, -1) // "ns:"
      return list.some(t => t.startsWith(prefix))
    }
    return list.includes(tagFilter.value)
  }

  const filtered = computed(() => {
    if (tagExprResults.value !== null) {
      let list = tagExprResults.value
      if (layerFilter.value !== 'all') list = list.filter(m => m.layer === layerFilter.value)
      if (tagFilter.value) list = list.filter(m => matchesTagFilter(m.tags))
      return list
    }
    let list = all.value
    if (searchQuery.value.trim()) {
      const q = searchQuery.value.toLowerCase()
      list = list.filter(m =>
        m.title.toLowerCase().includes(q) ||
        (m.content || '').toLowerCase().includes(q) ||
        (m.tags || []).some(t => t.toLowerCase().includes(q))
      )
    }
    if (layerFilter.value !== 'all') {
      list = list.filter(m => m.layer === layerFilter.value)
    }
    if (tagFilter.value) {
      list = list.filter(m => matchesTagFilter(m.tags))
    }
    return list
  })

  const dirty = computed(() => {
    if (!selected.value || !draft.value) return false
    return (
      draft.value.title !== selected.value.title ||
      draft.value.content !== selected.value.content ||
      JSON.stringify(draft.value.tags) !== JSON.stringify(selected.value.tags || [])
    )
  })

  // Whether a memory has unsaved edits — either it's the currently selected
  // one and dirty, or it has a stash from being switched away from earlier.
  function isDraft(id) {
    if (selected.value?.id === id) return dirty.value
    return Object.prototype.hasOwnProperty.call(stashedDrafts.value, id)
  }

  function persistDrafts() {
    localStorage.setItem(DRAFTS_KEY, JSON.stringify(stashedDrafts.value))
  }

  // Keeps the currently-open draft in the persisted stash live, not just at
  // switch-away time, so a page reload while still editing (never having
  // switched to another memory) doesn't lose the edit either.
  watch(
    [draft, selected, creatingNew],
    () => {
      if (creatingNew.value) {
        if (isNewDraftFilled(draft.value)) {
          stashedDrafts.value[NEW_DRAFT_KEY] = draft.value
        } else {
          delete stashedDrafts.value[NEW_DRAFT_KEY]
        }
        persistDrafts()
        return
      }
      if (!selected.value) return
      if (dirty.value) {
        stashedDrafts.value[selected.value.id] = draft.value
      } else {
        delete stashedDrafts.value[selected.value.id]
      }
      persistDrafts()
    },
    { deep: true }
  )

  // Discards the current draft's unsaved edits, reverting to the last-saved
  // content, and clears any stash so switching away/back won't resurrect it.
  function resetDraft() {
    if (!selected.value) return
    draft.value = {
      title: selected.value.title,
      content: selected.value.content,
      tags: [...(selected.value.tags || [])],
    }
    delete stashedDrafts.value[selected.value.id]
    persistDrafts()
  }

  async function fetchAll() {
    loading.value = true
    try {
      const data = await api.listMemories(1000)
      all.value = data.memories ?? []
    } finally {
      loading.value = false
    }
  }

  // Re-fetches without the loading flag, and without disturbing the
  // currently open item's unsaved edits or the list's scroll position —
  // used to reflect memories changed elsewhere (e.g. via MCP) in the background.
  async function refreshSilently() {
    const data = await api.listMemories(1000)
    all.value = data.memories ?? []
    if (!selected.value) return
    const match = all.value.find(m => m.id === selected.value.id)

    if (!match) {
      // Memory was deleted elsewhere.
      selected.value = null
      draft.value = null
      conflict.value = null
      return
    }

    if (!dirty.value) {
      selected.value = match
      draft.value = { title: match.title, content: match.content, tags: [...(match.tags || [])] }
      conflict.value = null
      return
    }

    // Dirty: only flag a conflict if the server version actually changed
    // underneath us (not just an unrelated field/other memory refreshing).
    // Leave `selected`/`draft` untouched so the diff the user is looking at
    // stays coherent until they explicitly resolve it.
    const changed =
      match.title !== selected.value.title ||
      match.content !== selected.value.content ||
      JSON.stringify(match.tags || []) !== JSON.stringify(selected.value.tags || [])
    if (changed) {
      conflict.value = match
    }
  }

  // User chose to discard their local draft and adopt the version that
  // changed elsewhere while they were editing.
  function resolveConflictLoadLatest() {
    if (!conflict.value) return
    selected.value = conflict.value
    draft.value = {
      title: conflict.value.title,
      content: conflict.value.content,
      tags: [...(conflict.value.tags || [])],
    }
    delete stashedDrafts.value[selected.value.id]
    persistDrafts()
    conflict.value = null
  }

  // User chose to keep editing their own draft — the baseline moves forward
  // to the external version (so `dirty`/Save reflect it correctly), but the
  // draft's actual text is untouched. Saving after this will overwrite the
  // external change, which is now an informed, explicit choice.
  function resolveConflictKeepMine() {
    if (!conflict.value) return
    selected.value = conflict.value
    conflict.value = null
  }

  // Stashes whatever is currently open — an existing memory's dirty edit,
  // or an in-progress new-memory draft — before navigating away from it, so
  // switching to another memory / another New Memory / another page and
  // coming back restores it instead of silently discarding it.
  function stashCurrentDraft() {
    if (creatingNew.value) {
      if (isNewDraftFilled(draft.value)) {
        stashedDrafts.value[NEW_DRAFT_KEY] = draft.value
      } else {
        delete stashedDrafts.value[NEW_DRAFT_KEY]
      }
      persistDrafts()
    } else if (selected.value && dirty.value) {
      stashedDrafts.value[selected.value.id] = draft.value
      persistDrafts()
    }
  }

  function select(entry) {
    stashCurrentDraft()
    conflict.value = null
    creatingNew.value = false
    selected.value = entry
    if (!entry) {
      draft.value = null
      return
    }
    draft.value = stashedDrafts.value[entry.id]
      ?? { title: entry.title, content: entry.content, tags: [...(entry.tags || [])] }
  }

  // Opens a draft in the detail panel for composing a new memory — nothing
  // is sent to the backend until Save. Restores a stashed new-memory draft
  // left behind from switching away earlier, if there is one; otherwise
  // starts empty. Stashes whatever was open before switching to this, same
  // as switching to another memory.
  function startNew() {
    stashCurrentDraft()
    conflict.value = null
    selected.value = null
    creatingNew.value = true
    draft.value = stashedDrafts.value[NEW_DRAFT_KEY]
      ?? { title: '', content: '', tags: [], layer: 'workspace' }
  }

  function cancelNew() {
    creatingNew.value = false
    draft.value = null
    delete stashedDrafts.value[NEW_DRAFT_KEY]
    persistDrafts()
  }

  async function save() {
    if (creatingNew.value) {
      if (!canSaveNew.value) return false
      saving.value = true
      try {
        const id = await create({
          title: draft.value.title.trim(),
          content: draft.value.content,
          tags: draft.value.tags,
          layer: draft.value.layer,
        })
        // create() -> select(created) re-stashes this draft under
        // NEW_DRAFT_KEY on the way (creatingNew was still true when it
        // ran) since it can't tell "switching away" from "just saved" —
        // clear it now that it's actually persisted as a real memory.
        delete stashedDrafts.value[NEW_DRAFT_KEY]
        persistDrafts()
        return !!id
      } finally {
        saving.value = false
      }
    }
    if (!selected.value || !dirty.value || conflict.value) return false
    saving.value = true
    try {
      const updated = await api.patchMemory(selected.value.id, {
        title: draft.value.title,
        content: draft.value.content,
        tags: draft.value.tags,
      })
      const idx = all.value.findIndex(m => m.id === selected.value.id)
      if (idx !== -1) all.value[idx] = updated
      selected.value = updated
      draft.value = { title: updated.title, content: updated.content, tags: [...(updated.tags || [])] }
      delete stashedDrafts.value[updated.id]
      persistDrafts()
      conflict.value = null
      return true
    } finally {
      saving.value = false
    }
  }

  // Folds a memory object that changed outside the normal edit flow (e.g.
  // an approved suggestion embedding a mention link) into `all`, and into
  // `selected`/`draft` too when it's the open one — reusing the same
  // not-dirty/dirty split as refreshSilently so an in-progress edit here
  // isn't silently clobbered, just flagged as a conflict instead.
  function applyExternalUpdate(updated) {
    const idx = all.value.findIndex(m => m.id === updated.id)
    if (idx !== -1) all.value[idx] = updated
    else all.value.unshift(updated)

    if (selected.value?.id !== updated.id) return
    if (!dirty.value) {
      selected.value = updated
      draft.value = { title: updated.title, content: updated.content, tags: [...(updated.tags || [])] }
      conflict.value = null
    } else {
      conflict.value = updated
    }
  }

  // Direct content patch outside the draft/save flow — used when approving
  // a suggested connection embeds a mention link into the source memory.
  async function patchContent(id, content) {
    const updated = await api.patchMemory(id, { content })
    applyExternalUpdate(updated)
    return updated
  }

  async function create({ title, content, tags, layer }) {
    const res = await api.createMemory({ title, content, tags, layer })
    await fetchAll()
    const created = all.value.find(m => m.id === res.id)
    if (created) select(created)
    return res.id
  }

  async function remove(id) {
    await api.deleteMemory(id)
    all.value = all.value.filter(m => m.id !== id)
    if (selected.value?.id === id) select(null)
    delete stashedDrafts.value[id]
    persistDrafts()
  }

  async function clearAll() {
    await api.deleteAllMemories()
    all.value = []
    stashedDrafts.value = {}
    persistDrafts()
    select(null)
  }

  return {
    all, selected, draft, conflict, searchQuery, layerFilter, loading, saving, filtered, dirty,
    tagExprResults, tagFilter,
    creatingNew, canSaveNew, hasNewDraft,
    isDraft, resetDraft, resolveConflictLoadLatest, resolveConflictKeepMine,
    fetchAll, refreshSilently, select, startNew, cancelNew, save, create, remove, clearAll,
    applyExternalUpdate, patchContent,
  }
})

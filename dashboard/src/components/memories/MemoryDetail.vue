<script setup>
import { ref, computed, nextTick, onMounted, onBeforeUnmount, watch } from 'vue'
import { PhDownloadSimple, PhFlag, PhShareNetwork } from '@phosphor-icons/vue'
import { useMemoriesStore } from '../../stores/memories.js'
import { useUiStore } from '../../stores/ui.js'
import { useGraphStore } from '../../stores/graph.js'
import LayerBadge from '../shared/LayerBadge.vue'
import TagInput from '../shared/TagInput.vue'
import EmptyState from '../shared/EmptyState.vue'
import DeleteConfirmModal from './DeleteConfirmModal.vue'
import MarkdownContent from '../shared/MarkdownContent.vue'
import ConfirmModal from '../shared/ConfirmModal.vue'
import CopyIdButton from '../shared/CopyIdButton.vue'
import { fmtDate, slugify } from '../../lib/format.js'
import { createFeedback } from '../../api/feedback.js'
import { countTokens } from '../../api/memories.js'
import { caretCoords } from '../../lib/caret.js'
import { diffPreview } from '../../lib/mention.js'

const memories = useMemoriesStore()
const ui = useUiStore()
const graph = useGraphStore()

const showDeleteModal = ref(false)
const showResetModal = ref(false)
const flagOpen = ref(false)
const contentView = ref('markdown') // 'markdown' | 'raw'

// New memories start in Raw view — there's no saved markdown to preview yet.
watch(() => memories.creatingNew, (isNew) => {
  if (isNew) contentView.value = 'raw'
})

// Downloading only makes sense for a memory that's actually saved on the
// server with no unsaved edits in flight — a new/unsaved draft has nothing
// durable to export yet.
const canDownload = computed(() => !!memories.selected && !memories.creatingNew && !memories.dirty)

function downloadMarkdown() {
  if (!canDownload.value) return
  const mem = memories.selected
  const blob = new Blob([mem.content], { type: 'text/markdown;charset=utf-8' })
  const url = URL.createObjectURL(blob)
  const a = document.createElement('a')
  a.href = url
  a.download = `${slugify(mem.title) || mem.id}.md`
  document.body.appendChild(a)
  a.click()
  a.remove()
  URL.revokeObjectURL(url)
}

// Live token count while editing — debounced so we're not hitting the API
// on every keystroke, but still feels live. Best-effort: a failed count
// request just leaves the last-known count stale, it's not on the save path.
const tokenCount = ref(0)
const maxContentTokens = ref(null)
const tokenOverLimit = computed(() => maxContentTokens.value != null && tokenCount.value > maxContentTokens.value)
let tokenDebounceTimer = null

async function refreshTokenCount() {
  if (!memories.draft) return
  try {
    const res = await countTokens({ title: memories.draft.title || '', content: memories.draft.content || '' })
    tokenCount.value = res.tokens
    maxContentTokens.value = res.max_content_tokens
  } catch {
    // best-effort — leave the last known count displayed
  }
}

watch(
  () => [memories.draft?.title, memories.draft?.content],
  () => {
    clearTimeout(tokenDebounceTimer)
    tokenDebounceTimer = setTimeout(refreshTokenCount, 300)
  },
  { immediate: true },
)

onBeforeUnmount(() => clearTimeout(tokenDebounceTimer))

const contentEl = ref(null)
const mention = ref(null) // { start, query, top, left } | null
const mentionIndex = ref(0)
const mentionStage = ref('search') // 'search' | 'kind'
const kindIndex = ref(0)
const pendingTarget = ref(null)
const KIND_OPTIONS = ['sibling', 'parent', 'child']

const mentionResults = computed(() => {
  if (!mention.value) return []
  const q = mention.value.query.toLowerCase()
  return memories.all
    .filter(m => m.id !== memories.selected?.id && m.title.toLowerCase().includes(q))
    .slice(0, 8)
})

function detectMention(ta) {
  const before = ta.value.slice(0, ta.selectionStart)
  const at = before.lastIndexOf('@')
  if (at === -1) { mention.value = null; return }
  const prev = at === 0 ? ' ' : before[at - 1]
  const query = before.slice(at + 1)
  if (!/\s/.test(prev) || /\s/.test(query) || query.length > 40) {
    mention.value = null
    return
  }
  const { top, left } = caretCoords(ta, at)
  mention.value = { start: at, query, top: top + 22, left }
  mentionIndex.value = 0
  mentionStage.value = 'search'
}

function onContentInput(e) {
  memories.draft.content = e.target.value
  detectMention(e.target)
}

function onContentKeydown(e) {
  if (!mention.value) return
  if (mentionStage.value === 'kind') {
    if (e.key === 'ArrowDown' || e.key === 'ArrowRight') {
      e.preventDefault()
      kindIndex.value = (kindIndex.value + 1) % KIND_OPTIONS.length
    } else if (e.key === 'ArrowUp' || e.key === 'ArrowLeft') {
      e.preventDefault()
      kindIndex.value = (kindIndex.value - 1 + KIND_OPTIONS.length) % KIND_OPTIONS.length
    } else if (e.key === 'Enter') {
      e.preventDefault()
      insertMention(pendingTarget.value, KIND_OPTIONS[kindIndex.value])
    } else if (e.key === 'Escape') {
      mention.value = null
    }
    return
  }
  if (!mentionResults.value.length) return
  const n = mentionResults.value.length
  if (e.key === 'ArrowDown') { e.preventDefault(); mentionIndex.value = (mentionIndex.value + 1) % n }
  else if (e.key === 'ArrowUp') { e.preventDefault(); mentionIndex.value = (mentionIndex.value - 1 + n) % n }
  else if (e.key === 'Enter') { e.preventDefault(); chooseTarget(mentionResults.value[mentionIndex.value]) }
  else if (e.key === 'Escape') { mention.value = null }
}

function chooseTarget(m) {
  pendingTarget.value = m
  mentionStage.value = 'kind'
  kindIndex.value = 0
}

function insertMention(m, kind) {
  const ta = contentEl.value
  const text = memories.draft.content
  const caret = ta.selectionStart
  // Strip brackets/parens from the link text: the backend's regex is a plain
  // regex (no backslash-escape awareness), so an unescaped `]`/`)` in the title
  // would prematurely terminate the `[phrase](kind:mem_id)` markdown link.
  const safeTitle = m.title.replace(/[[\]()]/g, '')
  const insert = kind === 'sibling' ? `[${safeTitle}](${m.id})` : `[${safeTitle}](${kind}:${m.id})`
  const pos = mention.value.start + insert.length
  memories.draft.content = text.slice(0, mention.value.start) + insert + text.slice(caret)
  mention.value = null
  mentionStage.value = 'search'
  nextTick(() => { ta.focus(); ta.setSelectionRange(pos, pos) })
}

// When a pending suggestion for the currently open memory is selected in
// SuggestPanel, preview what Approve would change right in the content
// field instead of a separate inline box.
const pendingDiff = computed(() => {
  const edge = graph.selectedEdge
  if (!edge || edge.status !== 'pending' || !memories.selected) return null
  if (edge.source_id !== memories.selected.id) return null
  return diffPreview(memories.selected.content, edge)
})

// edge.relationship describes the target relative to the source (e.g.
// "parent" means the target is the source's parent). When the memory we're
// viewing is the target, not the source, the label must be flipped so it
// still describes the OTHER memory correctly.
const INVERSE_RELATIONSHIP = { parent: 'child', child: 'parent', sibling: 'sibling' }
function relationshipFor(edge) {
  if (edge.source_id === memories.selected.id) return edge.relationship
  return INVERSE_RELATIONSHIP[edge.relationship] ?? edge.relationship
}

function goToMemory(id) {
  const target = memories.all.find(m => m.id === id)
  if (target) memories.select(target)
}

async function flag(signal) {
  flagOpen.value = false
  await createFeedback({ memory_id: memories.selected.id, signal })
  ui.showToast(`Flagged as ${signal}`)
}

async function handleSave() {
  const wasNew = memories.creatingNew
  const saved = await memories.save()
  if (saved) ui.showToast(wasNew ? 'Memory created' : 'Changes saved')
}

function handleCancelNew() {
  memories.cancelNew()
}

async function handleDelete() {
  const id = memories.selected.id
  showDeleteModal.value = false
  await memories.remove(id)
  ui.showToast('Memory deleted')
}

function openInGraph() {
  if (!memories.selected) return
  graph.selectedNodeId = memories.selected.id
  ui.requestActiveView('graph')
}

function handleReset() {
  showResetModal.value = false
  memories.resetDraft()
  ui.showToast('Changes reset')
}

function handleKeydown(e) {
  if ((e.ctrlKey || e.metaKey) && e.key === 's') {
    e.preventDefault()
    if (memories.saving) return
    if (memories.creatingNew ? memories.canSaveNew : memories.selected && memories.dirty) handleSave()
  }
}

onMounted(() => window.addEventListener('keydown', handleKeydown))
onBeforeUnmount(() => window.removeEventListener('keydown', handleKeydown))
</script>

<template>
  <div class="flex flex-col h-full flex-1" style="background:var(--hm-bg-base)">

    <!-- Empty state -->
    <EmptyState v-if="!memories.selected && !memories.creatingNew"
      message="Select a memory to view or edit"
      hint="Press / to search, Ctrl+S to save changes" />

    <template v-else>
      <!-- Toolbar -->
      <div class="flex items-center justify-between px-5 py-2.5"
        style="border-bottom:0.5px solid var(--hm-border-subtle)">
        <span class="flex items-center gap-1 font-mono" style="font-size:10px; color:var(--hm-text-tertiary)">
          {{ memories.creatingNew ? 'New memory (unsaved)' : memories.selected.id }}
          <CopyIdButton v-if="!memories.creatingNew" :id="memories.selected.id" />
        </span>
        <div v-if="memories.selected" class="flex gap-1">
          <button v-if="!memories.creatingNew" class="hm-btn hm-btn-ghost hm-btn-sm" title="View in graph" @click="openInGraph">
            <PhShareNetwork :size="14" /> Go to graph
          </button>
          <button class="hm-btn hm-btn-ghost hm-btn-sm"
            :disabled="!canDownload"
            :title="memories.dirty ? 'Save changes before downloading' : 'Download as Markdown'"
            @click="downloadMarkdown">
            <PhDownloadSimple :size="14" /> Download
          </button>
          <div class="relative">
            <button class="hm-btn hm-btn-ghost hm-btn-sm" title="Flag for review"
              @click="flagOpen = !flagOpen" @keydown.esc="flagOpen = false">
              <PhFlag :size="14" />
            </button>
            <div v-if="flagOpen" class="fixed inset-0" style="z-index:9" @click="flagOpen = false"></div>
            <div v-if="flagOpen" class="absolute right-0 mt-1 rounded-md py-1"
              style="background:var(--hm-bg-overlay); border:0.5px solid var(--hm-border-default); z-index:10">
              <button v-for="r in ['incorrect','outdated','duplicate','other']" :key="r"
                class="flag-option block w-full text-left px-3 py-1.5"
                @click="flag(r)">{{ r }}</button>
            </div>
          </div>
          <button class="hm-btn hm-btn-danger hm-btn-sm" @click="showDeleteModal = true">Delete</button>
        </div>
      </div>

      <!-- Conflict: updated elsewhere while this draft was open -->
      <div v-if="memories.conflict" class="conflict-banner">
        <p class="conflict-banner__title">This memory was updated elsewhere while you were editing.</p>
        <p class="conflict-banner__body">
          Someone (or an agent) saved a different version of "{{ memories.conflict.title }}" while your changes here were unsaved. Choose which version to keep.
        </p>
        <div class="flex items-center gap-2 mt-2">
          <button class="hm-btn hm-btn-primary hm-btn-sm" @click="memories.resolveConflictLoadLatest()">
            Load latest version
          </button>
          <button class="hm-btn hm-btn-default hm-btn-sm" @click="memories.resolveConflictKeepMine()">
            Keep my draft
          </button>
        </div>
      </div>

      <!-- Body -->
      <div class="flex-1 overflow-y-auto px-6 py-5">
        <!-- Title -->
        <label class="hm-label" for="mem-title">TITLE</label>
        <input
          id="mem-title"
          class="mem-title-input mb-6"
          placeholder="Untitled memory"
          :value="memories.draft?.title"
          @input="memories.draft.title = $event.target.value"
        />

        <!-- Content -->
        <div class="flex items-center justify-between mb-1.5">
          <label class="hm-label" style="margin-bottom:0" for="mem-content">CONTENT</label>
          <div class="flex items-center gap-3">
            <span v-if="maxContentTokens != null" class="font-mono"
              :title="tokenOverLimit ? 'Exceeds max_content_tokens — split into an index memory plus fragments before saving' : 'Title + content token count'"
              :style="`font-size:10px; color:${tokenOverLimit ? 'var(--hm-danger)' : 'var(--hm-text-tertiary)'}`">
              {{ tokenCount }}/{{ maxContentTokens }}
            </span>
            <div class="content-toggle" role="tablist" aria-label="Content view">
              <button type="button" role="tab" :aria-selected="contentView === 'markdown'"
                class="content-toggle__btn" :class="{ 'content-toggle__btn--active': contentView === 'markdown' }"
                @click="contentView = 'markdown'">Markdown</button>
              <button type="button" role="tab" :aria-selected="contentView === 'raw'"
                class="content-toggle__btn" :class="{ 'content-toggle__btn--active': contentView === 'raw' }"
                @click="contentView = 'raw'">Raw</button>
            </div>
          </div>
        </div>
        <div v-if="pendingDiff" class="mb-6 rounded"
          style="font-family:var(--hm-font-mono); font-size:12px; line-height:1.6; padding:12px 14px;
                 background:var(--hm-mono-bg); border:0.5px solid var(--hm-border-subtle)">
          <div style="color:var(--hm-text-secondary); white-space:pre-wrap; word-break:break-word">{{ pendingDiff.context }}</div>
          <div v-if="pendingDiff.removed" style="color:var(--hm-danger); text-decoration:line-through; white-space:pre-wrap; word-break:break-word">- {{ pendingDiff.removed }}</div>
          <div style="color:var(--hm-success); white-space:pre-wrap; word-break:break-word">+ {{ pendingDiff.added }}</div>
        </div>

        <div v-else-if="contentView === 'raw'" class="relative mb-6">
          <textarea
            id="mem-content"
            ref="contentEl"
            class="hm-input resize-none w-full"
            style="height:40vh; min-height:160px; padding:10px 12px; font-family:var(--hm-font-mono); font-size:12px; line-height:1.6; background:var(--hm-mono-bg)"
            :value="memories.draft?.content"
            @input="onContentInput"
            @keydown="onContentKeydown"
            @blur="mention = null; mentionStage = 'search'"
          ></textarea>
          <div v-if="mention && mentionStage === 'search' && mentionResults.length" class="mention-menu"
            :style="{ top: mention.top + 'px', left: mention.left + 'px' }">
            <button v-for="(m, i) in mentionResults" :key="m.id" type="button"
              class="mention-menu__item" :class="{ 'mention-menu__item--active': i === mentionIndex }"
              @mousedown.prevent="chooseTarget(m)">
              {{ m.title }}
            </button>
          </div>
          <div v-if="mention && mentionStage === 'kind'" class="mention-menu"
            :style="{ top: mention.top + 'px', left: mention.left + 'px' }">
            <button v-for="(opt, i) in KIND_OPTIONS" :key="opt" type="button"
              class="mention-menu__item" :class="{ 'mention-menu__item--active': i === kindIndex }"
              @mousedown.prevent="insertMention(pendingTarget, opt)">
              {{ opt }}
            </button>
          </div>
        </div>
        <div v-else id="mem-content" class="mb-6 overflow-y-auto"
          style="height:40vh; min-height:160px; padding:10px 12px; border-radius:6px; border:0.5px solid var(--hm-border-default); background:var(--hm-mono-bg)">
          <MarkdownContent :text="memories.draft?.content" @navigate="goToMemory" />
        </div>

        <!-- Tags -->
        <label class="hm-label" id="mem-tags-label">TAGS</label>
        <div class="mb-6" aria-labelledby="mem-tags-label">
          <TagInput
            :model-value="memories.draft?.tags ?? []"
            @update:model-value="memories.draft.tags = $event" />
        </div>

        <!-- Layer: only chosen once, at creation -->
        <template v-if="memories.creatingNew">
          <label class="hm-label">LAYER</label>
          <div class="flex gap-1.5 mb-6">
            <button class="hm-btn hm-btn-sm"
              :style="memories.draft.layer==='workspace' ? 'background:var(--hm-workspace-bg); border-color:var(--hm-workspace); color:var(--hm-workspace)' : 'border-color:var(--hm-border-subtle); color:var(--hm-text-secondary)'"
              @click="memories.draft.layer='workspace'">workspace</button>
            <button class="hm-btn hm-btn-sm"
              :style="memories.draft.layer==='personal' ? 'background:var(--hm-personal-bg); border-color:var(--hm-personal); color:var(--hm-personal)' : 'border-color:var(--hm-border-subtle); color:var(--hm-text-secondary)'"
              @click="memories.draft.layer='personal'">personal</button>
            <button class="hm-btn hm-btn-sm"
              :disabled="!ui.orgInfo?.configured"
              :title="ui.orgInfo?.configured ? '' : 'Org layer not configured — set [org_sync] in the global config'"
              :style="memories.draft.layer==='org' ? 'background:var(--hm-org-bg); border-color:var(--hm-org); color:var(--hm-org)' : 'border-color:var(--hm-border-subtle); color:var(--hm-text-secondary)'"
              @click="memories.draft.layer='org'">org</button>
          </div>
        </template>

        <!-- Connections -->
        <template v-if="memories.selected && graph.edgesFor(memories.selected.id).length">
          <label class="hm-label">CONNECTIONS · {{ graph.edgesFor(memories.selected.id).length }}</label>
          <div class="flex flex-col gap-1.5 mb-2">
            <div v-for="edge in graph.edgesFor(memories.selected.id)" :key="edge.id"
              class="conn-row"
              @click="memories.select(memories.all.find(m => m.id === (edge.source_id === memories.selected.id ? edge.target_id : edge.source_id)))">
              <span class="font-mono conn-row__rel">
                {{ relationshipFor(edge) }}
              </span>
              <span class="conn-row__title">
                {{ memories.all.find(m => m.id === (edge.source_id === memories.selected.id ? edge.target_id : edge.source_id))?.title || edge.target_id }}
              </span>
              <span class="conn-row__arrow">→</span>
            </div>
          </div>
        </template>
      </div>

      <!-- Footer -->
      <div class="flex items-center justify-between px-5"
        style="height:40px; border-top:0.5px solid var(--hm-border-subtle)">
        <span v-if="memories.selected" class="flex items-center gap-2 font-mono"
          style="font-size:11px; color:var(--hm-text-tertiary)">
          updated {{ fmtDate(memories.selected.updated_at) }}
          <LayerBadge :layer="memories.selected.layer" />
        </span>
        <span v-else class="font-mono" style="font-size:11px; color:var(--hm-text-tertiary)">
          not saved yet
        </span>
        <div class="flex items-center gap-2">
          <button v-if="memories.creatingNew" class="hm-btn hm-btn-default hm-btn-sm"
            :disabled="memories.saving"
            @click="handleCancelNew">
            Cancel
          </button>
          <button v-else-if="memories.dirty" class="hm-btn hm-btn-default hm-btn-sm"
            :disabled="memories.saving"
            @click="showResetModal = true">
            Reset
          </button>
          <button class="hm-btn hm-btn-primary hm-btn-sm"
            :disabled="(memories.creatingNew ? !memories.canSaveNew : !memories.dirty) || memories.saving || !!memories.conflict"
            :title="memories.conflict ? 'Resolve the conflict above before saving' : undefined"
            @click="handleSave">
            {{ memories.saving ? (memories.creatingNew ? 'Creating…' : 'Saving…') : (memories.creatingNew ? 'Create' : 'Save') }}
          </button>
        </div>
      </div>
    </template>

    <DeleteConfirmModal
      v-if="showDeleteModal"
      :mem="memories.selected"
      @confirm="handleDelete"
      @cancel="showDeleteModal = false"
    />

    <ConfirmModal
      v-if="showResetModal"
      title="Reset changes?"
      body="Unsaved edits to this memory will be discarded and reverted to the last saved version. This cannot be undone."
      confirmLabel="Reset"
      :dangerous="true"
      @confirm="handleReset"
      @cancel="showResetModal = false"
    />
  </div>
</template>

<style scoped>
.mem-title-input {
  display: block;
  width: 100%;
  font-family: var(--hm-font-sans);
  font-size: 20px;
  font-weight: 700;
  letter-spacing: -0.02em;
  line-height: 1.3;
  color: var(--hm-text-primary);
  background: transparent;
  border: 0.5px solid transparent;
  border-radius: 6px;
  padding: 4px 8px;
  margin: -4px -8px;
  outline: none;
  transition: border-color 0.1s, background 0.1s;
}

.mem-title-input::placeholder {
  color: var(--hm-text-tertiary);
  font-weight: 700;
}

.mem-title-input:hover {
  border-color: var(--hm-border-subtle);
}

.mem-title-input:focus {
  border-color: var(--hm-accent);
  background: var(--hm-bg-elevated);
}

.conn-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 9px 12px;
  border-radius: 6px;
  border: 0.5px solid var(--hm-border-subtle);
  font-size: 12px;
  cursor: pointer;
  transition: border-color 0.1s, background 0.1s;
}

.conn-row:hover {
  border-color: var(--hm-border-default);
  background: var(--hm-bg-elevated);
}

.conn-row__rel {
  flex-shrink: 0;
  font-size: 10px;
  color: var(--hm-text-tertiary);
  background: var(--hm-bg-elevated);
  border: 0.5px solid var(--hm-border-subtle);
  border-radius: 4px;
  padding: 2px 6px;
}

.conn-row__title {
  flex: 1;
  min-width: 0;
  color: var(--hm-text-primary);
  overflow: hidden;
  text-overflow: ellipsis;
  white-space: nowrap;
}

.conn-row__arrow {
  flex-shrink: 0;
  color: var(--hm-text-tertiary);
}

.flag-option {
  font-size: 12px;
  color: var(--hm-text-secondary);
  background: none;
  border: none;
  cursor: pointer;
}

.flag-option:hover,
.flag-option:focus-visible {
  background: var(--hm-bg-elevated);
  color: var(--hm-text-primary);
  outline: none;
}

.content-toggle {
  display: flex;
  gap: 2px;
  padding: 2px;
  border-radius: 6px;
  background: var(--hm-bg-elevated);
}

.content-toggle__btn {
  font-size: 10px;
  font-weight: 500;
  padding: 3px 8px;
  border-radius: 4px;
  border: none;
  background: transparent;
  color: var(--hm-text-tertiary);
  cursor: pointer;
  transition: background 0.1s, color 0.1s;
}

.content-toggle__btn:hover {
  color: var(--hm-text-primary);
}

.content-toggle__btn:focus-visible {
  outline: 2px solid var(--hm-accent);
  outline-offset: -2px;
}

.content-toggle__btn--active {
  background: var(--hm-bg-overlay);
  color: var(--hm-text-primary);
}

.mention-menu {
  position: absolute;
  z-index: 20;
  min-width: 180px;
  max-width: 320px;
  padding: 4px 0;
  border-radius: 6px;
  background: var(--hm-bg-overlay);
  border: 0.5px solid var(--hm-border-default);
  box-shadow: 0 4px 16px rgba(0, 0, 0, 0.25);
}
.mention-menu__item {
  display: block;
  width: 100%;
  text-align: left;
  padding: 5px 10px;
  font-size: 12px;
  color: var(--hm-text-secondary);
  background: none;
  border: none;
  cursor: pointer;
  white-space: nowrap;
  overflow: hidden;
  text-overflow: ellipsis;
}
.mention-menu__item--active,
.mention-menu__item:hover {
  background: var(--hm-bg-elevated);
  color: var(--hm-text-primary);
}

.conflict-banner {
  margin: 12px 20px 0;
  padding: 12px 14px;
  border-radius: 8px;
  background: var(--hm-warning-bg);
  border: 0.5px solid var(--hm-warning-border);
}

.conflict-banner__title {
  font-size: 12px;
  font-weight: 600;
  color: var(--hm-warning);
  margin-bottom: 4px;
}

.conflict-banner__body {
  font-size: 11px;
  color: var(--hm-text-secondary);
}
</style>

<script setup>
import { ref, computed, watch, onBeforeUnmount } from 'vue'
import { PhX } from '@phosphor-icons/vue'
import { useAuditStore } from '../../stores/audit.js'
import { useGraphStore } from '../../stores/graph.js'
import { useMemoriesStore } from '../../stores/memories.js'
import { lineDiff } from '../../lib/lineDiff.js'

const audit = useAuditStore()
const graph = useGraphStore()
const memories = useMemoriesStore()

const expanded = ref(null)    // proposal id whose details are open
const revisingFor = ref(null) // proposal id whose revise textbox is open
const feedbackText = ref('')

const projects = computed(() => [...new Set(
  memories.all.flatMap(m => (m.tags || []).filter(t => t.startsWith('project:')).map(t => t.slice('project:'.length))),
)].sort())
watch(projects, ps => {
  if (!audit.project && ps.length) audit.project = ps[0]
}, { immediate: true })

const visible = computed(() => audit.proposals.filter(p => !audit.project || p.project === audit.project))

// Elapsed-time display while the agent works; stops when the phase moves on.
const now = ref(Date.now())
let tickTimer = null
watch(() => audit.phase, (phase) => {
  if (phase === 'auditing') {
    now.value = Date.now()
    if (!tickTimer) tickTimer = setInterval(() => { now.value = Date.now() }, 1000)
  } else if (tickTimer) {
    clearInterval(tickTimer)
    tickTimer = null
  }
}, { immediate: true })
onBeforeUnmount(() => { if (tickTimer) clearInterval(tickTimer) })
const elapsedSeconds = computed(() =>
  audit.auditingStartedAt ? Math.max(0, Math.floor((now.value - audit.auditingStartedAt) / 1000)) : 0)

function titleOf(id) {
  return memories.all.find(m => m.id === id)?.title ?? id
}

// For an update op: the memory's current content against the proposed one.
function diffFor(op) {
  const current = memories.all.find(m => m.id === op.id)
  return lineDiff(current?.content, op.content ?? current?.content)
}

function rowState(p) {
  if (audit.revisingProposalId === p.id) return 'revising'
  if (audit.queuedProposalIds.includes(p.id)) return 'queued'
  return 'idle'
}

function submitRevise(p) {
  const text = feedbackText.value.trim()
  if (!text) return
  audit.revise(p.id, text)
  revisingFor.value = null
  feedbackText.value = ''
}

const legacyRows = computed(() => graph.pendingEdges.map(e => ({
  ...e, sourceTitle: titleOf(e.source_id), targetTitle: titleOf(e.target_id),
})))
</script>

<template>
  <div class="flex flex-col h-full shrink-0"
    style="width:340px; background:var(--hm-bg-surface); border-left:0.5px solid var(--hm-border-subtle)">

    <div class="flex items-center justify-between px-4"
      style="height:52px; border-bottom:0.5px solid var(--hm-border-subtle)">
      <span style="font-size:13px; font-weight:500; color:var(--hm-text-primary)">Audit</span>
      <button class="hm-btn hm-btn-ghost hm-btn-sm" aria-label="Close" @click="audit.closePanel()">
        <PhX :size="14" weight="bold" />
      </button>
    </div>

    <div class="px-4 py-2 flex items-center gap-1.5" style="border-bottom:0.5px solid var(--hm-border-subtle)">
      <select v-model="audit.project" class="hm-input" style="height:26px; font-size:12px">
        <option v-for="p in projects" :key="p" :value="p">{{ p }}</option>
      </select>
      <button v-if="!audit.active" class="hm-btn hm-btn-primary hm-btn-sm" :disabled="!audit.project"
        @click="audit.start(audit.project)">Run audit</button>
      <button v-else class="hm-btn hm-btn-default hm-btn-sm" @click="audit.end()">End session</button>
    </div>

    <div v-if="audit.error" class="px-4 py-2"
      style="font-size:11px; color:var(--hm-warning); background:var(--hm-warning-bg)">
      {{ audit.error }}
    </div>

    <div v-if="audit.phase === 'auditing'" class="px-4 py-3 flex items-center gap-2"
      style="font-size:13px; font-weight:600; color:var(--hm-text-primary)">
      <span class="hm-skeleton" style="display:inline-block; width:10px; height:10px; border-radius:50%"></span>
      Agent is auditing {{ audit.project }}…
      <span class="font-mono" style="margin-left:auto; font-weight:500; font-size:11px; color:var(--hm-text-tertiary)">
        {{ elapsedSeconds }}s
      </span>
    </div>

    <div class="flex-1 overflow-y-auto">
      <p v-if="!visible.length && audit.phase !== 'auditing'" class="px-4 py-3"
        style="font-size:12px; color:var(--hm-text-tertiary)">
        No pending fixes for {{ audit.project || 'this project' }}.
      </p>

      <div v-for="p in visible" :key="p.id" class="px-4 py-3"
        style="border-bottom:0.5px solid var(--hm-border-subtle)">
        <p class="flex items-center gap-1.5" style="font-size:12px; color:var(--hm-text-primary)">
          <span class="font-mono rounded-sm px-1" style="font-size:9px; background:var(--hm-bg-elevated); color:var(--hm-text-secondary)">{{ p.kind }}</span>
          {{ p.summary }}
        </p>
        <p class="mt-1" style="font-size:11px; color:var(--hm-text-secondary)">{{ p.reason }}</p>

        <div v-if="expanded === p.id" class="mt-2 flex flex-col gap-2" style="font-size:11px">
          <div v-for="(op, i) in p.ops" :key="i" class="rounded-sm p-2" style="background:var(--hm-bg-base)">
            <template v-if="op.op === 'create'">
              <p style="color:var(--hm-success)">New: {{ op.title }}</p>
              <p v-if="op.parent" style="color:var(--hm-text-secondary)">under {{ titleOf(op.parent) }}: {{ op.pointer }}</p>
              <p style="color:var(--hm-text-tertiary)">{{ (op.tags || []).join(', ') }}</p>
              <pre class="whitespace-pre-wrap font-mono mt-1" style="color:var(--hm-text-primary)">{{ op.content }}</pre>
            </template>
            <template v-else>
              <p style="color:var(--hm-text-primary)">Change: {{ titleOf(op.id) }}</p>
              <p v-if="op.tags" style="color:var(--hm-text-tertiary)">tags: {{ op.tags.join(', ') }}</p>
              <pre v-for="line in diffFor(op).removed" :key="'r' + line"
                class="whitespace-pre-wrap font-mono" style="color:var(--hm-danger)">{{ line }}</pre>
              <pre v-for="(line, j) in diffFor(op).lines" :key="'l' + j"
                class="whitespace-pre-wrap font-mono"
                :style="line.added ? 'color:var(--hm-success)' : 'color:var(--hm-text-tertiary)'">{{ line.text }}</pre>
            </template>
          </div>
        </div>

        <div v-if="rowState(p) === 'revising'" class="mt-2 flex items-center gap-1.5"
          style="font-size:11px; color:var(--hm-warning)">
          <span class="hm-skeleton" style="display:inline-block; width:8px; height:8px; border-radius:50%"></span>
          revising…
        </div>
        <div v-else-if="rowState(p) === 'queued'" class="mt-2" style="font-size:11px; color:var(--hm-text-tertiary)">
          queued
        </div>
        <div v-else class="mt-2 flex gap-1.5">
          <button v-if="p.ops.length" class="hm-btn hm-btn-primary hm-btn-sm" @click="audit.apply(p.id)">Approve</button>
          <button class="hm-btn hm-btn-default hm-btn-sm" @click="audit.reject(p.id)">{{ p.ops.length ? 'Reject' : 'Dismiss' }}</button>
          <button v-if="p.ops.length" class="hm-btn hm-btn-ghost hm-btn-sm"
            @click="expanded = expanded === p.id ? null : p.id">Details</button>
          <button v-if="audit.active" class="hm-btn hm-btn-ghost hm-btn-sm"
            @click="revisingFor = p.id; feedbackText = ''">Revise</button>
        </div>
        <input v-if="revisingFor === p.id" v-model="feedbackText"
          class="hm-input mt-2 w-full" placeholder="What should change? Enter to send"
          @keyup.enter="submitRevise(p)" @keyup.esc="revisingFor = null" />
      </div>

      <template v-if="legacyRows.length">
        <p class="px-4 pt-4 pb-1 font-mono" style="font-size:10px; color:var(--hm-text-tertiary)">
          Earlier connection suggestions
        </p>
        <div v-for="edge in legacyRows" :key="edge.id" class="px-4 py-3"
          style="border-bottom:0.5px solid var(--hm-border-subtle)">
          <p style="font-size:12px; color:var(--hm-text-primary)">
            {{ edge.sourceTitle }}
            <span class="font-mono" style="font-size:10px; color:var(--hm-warning)">--[{{ edge.relationship }}]--></span>
            {{ edge.targetTitle }}
          </p>
          <p v-if="edge.reason" class="mt-1" style="font-size:11px; color:var(--hm-text-secondary)">{{ edge.reason }}</p>
          <div class="mt-2 flex gap-1.5">
            <button class="hm-btn hm-btn-primary hm-btn-sm" @click="graph.resolveEdge(edge.id, 'active')">Approve</button>
            <button class="hm-btn hm-btn-default hm-btn-sm" @click="graph.resolveEdge(edge.id, 'rejected')">Reject</button>
          </div>
        </div>
      </template>
    </div>
  </div>
</template>

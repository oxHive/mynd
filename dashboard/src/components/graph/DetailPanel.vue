<script setup>
import { computed } from 'vue'
import { PhX, PhArrowSquareOut, PhFlag } from '@phosphor-icons/vue'
import { useGraphStore } from '../../stores/graph.js'
import { useMemoriesStore } from '../../stores/memories.js'
import { useUiStore } from '../../stores/ui.js'
import LayerBadge from '../shared/LayerBadge.vue'
import TagChip from '../shared/TagChip.vue'
import CopyButton from '../shared/CopyButton.vue'
import CopyIdButton from '../shared/CopyIdButton.vue'
import { createFeedback } from '../../api/feedback.js'
import { ref } from 'vue'

const graph = useGraphStore()
const memories = useMemoriesStore()
const ui = useUiStore()

const node = computed(() => memories.all.find(m => m.id === graph.selectedNodeId))

const INVERSE_RELATIONSHIP = { parent: 'child', child: 'parent', sibling: 'sibling' }
function relationshipFor(edge) {
  if (edge.source_id === node.value.id) return edge.relationship
  return INVERSE_RELATIONSHIP[edge.relationship] ?? edge.relationship
}
function otherMemory(edge) {
  const otherId = edge.source_id === node.value.id ? edge.target_id : edge.source_id
  return memories.all.find(m => m.id === otherId)
}
function goToConnection(edge) {
  const other = otherMemory(edge)
  if (other) graph.selectedNodeId = other.id
}

function openInMemories() {
  if (!node.value) return
  ui.requestActiveView('memories')
  memories.select(node.value)
}

const flagOpen = ref(false)
async function flag(signal) {
  flagOpen.value = false
  await createFeedback({ memory_id: node.value.id, signal })
  ui.showToast(`Flagged as ${signal}`)
}
</script>

<template>
  <Transition name="panel">
    <div v-show="graph.selectedNodeId" class="flex flex-col h-full shrink-0"
      style="width:280px; background:var(--hm-bg-surface); border-left:0.5px solid var(--hm-border-subtle)">

      <div class="flex items-center justify-between px-5 py-2.5"
        style="border-bottom:0.5px solid var(--hm-border-subtle)">
        <span style="font-size:13px; font-weight:500; color:var(--hm-text-primary)">Memory</span>
        <button class="hm-btn hm-btn-ghost hm-btn-sm" aria-label="Close" @click="graph.selectedNodeId = null">
          <PhX :size="14" weight="bold" />
        </button>
      </div>

      <div v-if="node" class="flex-1 overflow-y-auto px-5 py-4">
        <div class="flex items-center gap-1.5 mb-2.5">
          <LayerBadge :layer="node.layer" />
          <TagChip v-if="node.tags?.[0]" :tag="node.tags[0]" />
        </div>

        <div style="font-size:15px; font-weight:600; color:var(--hm-text-primary); line-height:1.3" class="mb-1.5">
          {{ node.title }}
        </div>

        <div class="flex items-center gap-1 mb-3">
          <span class="font-mono" style="font-size:10px; color:var(--hm-text-tertiary)">{{ node.id }}</span>
          <CopyIdButton :id="node.id" />
        </div>

        <p style="font-size:12px; color:var(--hm-text-secondary); line-height:1.55" class="mb-5">
          {{ node.content?.slice(0, 260) }}{{ (node.content?.length || 0) > 260 ? '…' : '' }}
        </p>

        <label class="hm-label">TAGS</label>
        <div class="flex flex-wrap gap-1.5 mb-5">
          <TagChip v-for="tag in node.tags" :key="tag" :tag="tag" />
        </div>

        <template v-if="graph.edgesFor(node.id).length">
          <label class="hm-label">CONNECTIONS · {{ graph.edgesFor(node.id).length }}</label>
          <div class="flex flex-col gap-1.5">
            <div v-for="edge in graph.edgesFor(node.id)" :key="edge.id" class="conn-row" @click="goToConnection(edge)">
              <span class="font-mono conn-row__rel">{{ relationshipFor(edge) }}</span>
              <span class="conn-row__title">{{ otherMemory(edge)?.title || edge.target_id }}</span>
            </div>
          </div>
        </template>
      </div>

      <div v-if="node" class="flex flex-col gap-2 px-5 py-3"
        style="border-top:0.5px solid var(--hm-border-subtle)">
        <button class="hm-btn hm-btn-sm connect-btn w-full justify-center" @click="openInMemories">
          <PhArrowSquareOut :size="13" weight="bold" /> Go to memory detail
        </button>
        <div class="flex items-center gap-2">
          <CopyButton :command="`/memory-edit ${node.id}`" label="/memory-edit" />
          <div class="relative ml-auto">
            <button class="hm-btn hm-btn-ghost hm-btn-sm" title="Flag for review"
              @click="flagOpen = !flagOpen" @keydown.esc="flagOpen = false">
              <PhFlag :size="14" />
            </button>
            <div v-if="flagOpen" class="fixed inset-0" style="z-index:9" @click="flagOpen = false"></div>
            <div v-if="flagOpen" class="absolute right-0 bottom-full mb-1 rounded-md py-1"
              style="background:var(--hm-bg-overlay); border:0.5px solid var(--hm-border-default); z-index:10; min-width:110px">
              <button v-for="r in ['incorrect','outdated','duplicate','other']" :key="r"
                class="flag-option block w-full text-left px-3 py-1.5" @click="flag(r)">{{ r }}</button>
            </div>
          </div>
        </div>
      </div>
    </div>
  </Transition>
</template>

<style scoped>
.panel-enter-active, .panel-leave-active { transition: transform 0.2s, opacity 0.2s; }
.panel-enter-from, .panel-leave-to { transform: translateX(20px); opacity: 0; }

.conn-row {
  display: flex;
  align-items: center;
  gap: 8px;
  padding: 8px 10px;
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

.connect-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  background: var(--hm-accent);
  border-color: var(--hm-accent);
  color: var(--hm-bg-base);
  font-weight: 600;
}

.connect-btn:hover {
  background: color-mix(in srgb, var(--hm-accent) 88%, white);
}

.flag-option {
  display: block;
  width: 100%;
  text-align: left;
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
</style>

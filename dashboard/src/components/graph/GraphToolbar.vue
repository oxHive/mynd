<script setup>
import { computed } from 'vue'
import { PhMagnifyingGlass, PhMinus, PhPlus, PhCornersOut, PhSparkle, PhArrowsClockwise } from '@phosphor-icons/vue'
import { useGraphStore } from '../../stores/graph.js'
import { useMemoriesStore } from '../../stores/memories.js'
import { useSuggestStore } from '../../stores/suggest.js'
import { useUiStore } from '../../stores/ui.js'
import TagFilter from '../shared/TagFilter.vue'

const graph = useGraphStore()
const memories = useMemoriesStore()
const suggest = useSuggestStore()
const ui = useUiStore()

const ALL_LAYER_FILTERS = [
  { label: 'All', value: 'all' },
  { label: 'Personal', value: 'personal' },
  { label: 'Workspace', value: 'workspace' },
  { label: 'Org', value: 'org' },
]

// Org filter only makes sense when the server actually has [org_sync]
// configured — otherwise it's a picker for a layer nothing can ever be in.
const LAYER_FILTERS = computed(() =>
  ui.orgInfo?.configured ? ALL_LAYER_FILTERS : ALL_LAYER_FILTERS.filter(f => f.value !== 'org')
)

function jumpToMatch() {
  const q = graph.searchQuery.trim().toLowerCase()
  if (!q) return
  const match = memories.all.find(m => m.title.toLowerCase().includes(q))
  if (match) graph.selectedNodeId = match.id
}

function resetZoom() {
  graph.zoom = 2
}
</script>

<template>
  <div class="flex items-center gap-3 px-4 shrink-0"
    style="height:52px; border-bottom:0.5px solid var(--hm-border-subtle); background:var(--hm-bg-surface)">

    <label class="toolbar-search">
      <PhMagnifyingGlass :size="14" weight="bold" />
      <input placeholder="Find node…" v-model="graph.searchQuery" @keyup.enter="jumpToMatch" />
    </label>

    <div class="layer-switch" role="tablist" aria-label="Filter by layer">
      <button v-for="f in LAYER_FILTERS" :key="f.value" role="tab" class="layer-switch__item"
        :class="{ 'layer-switch__item--active': graph.layerFilter === f.value }"
        :aria-selected="graph.layerFilter === f.value"
        @click="graph.layerFilter = f.value">
        {{ f.label }}
      </button>
    </div>

    <TagFilter v-model="graph.tagFilter" />

    <div class="flex items-center gap-2 ml-auto">
      <div class="zoom-group">
        <button class="zoom-group__btn" aria-label="Zoom out" @click="graph.zoom = Math.max(1, graph.zoom - 1)">
          <PhMinus :size="12" weight="bold" />
        </button>
        <span class="zoom-group__label font-mono">L{{ graph.zoom }}</span>
        <button class="zoom-group__btn" aria-label="Reset zoom" @click="resetZoom">
          <PhCornersOut :size="12" weight="bold" />
        </button>
        <button class="zoom-group__btn" aria-label="Zoom in" @click="graph.zoom = Math.min(3, graph.zoom + 1)">
          <PhPlus :size="12" weight="bold" />
        </button>
      </div>
      <button class="hm-btn hm-btn-ghost hm-btn-sm" title="Re-layout graph" @click="graph.requestRelayout()">
        <PhArrowsClockwise :size="13" weight="bold" />
      </button>
      <button class="hm-btn hm-btn-sm suggest-btn"
        @click="suggest.active ? suggest.openPanel() : suggest.start()">
        <PhSparkle :size="13" weight="fill" />
        {{ suggest.phase === 'suggesting' ? 'suggesting…' : 'suggest' }}
      </button>
    </div>
  </div>
</template>

<style scoped>
.toolbar-search {
  display: flex;
  align-items: center;
  gap: 7px;
  width: 190px;
  height: 30px;
  padding: 0 10px;
  border-radius: 7px;
  border: 0.5px solid var(--hm-border-subtle);
  background: var(--hm-bg-elevated);
  color: var(--hm-text-tertiary);
}

.toolbar-search:focus-within {
  border-color: var(--hm-accent);
  color: var(--hm-text-secondary);
}

.toolbar-search input {
  flex: 1;
  min-width: 0;
  border: none;
  background: transparent;
  outline: none;
  font-size: var(--hm-text-base);
  font-family: var(--hm-font-sans);
  color: var(--hm-text-primary);
}

.toolbar-search input::placeholder {
  color: var(--hm-text-tertiary);
}

.layer-switch {
  display: flex;
  align-items: center;
  gap: 2px;
  padding: 2px;
  border-radius: 7px;
  background: var(--hm-bg-elevated);
  border: 0.5px solid var(--hm-border-subtle);
}

.layer-switch__item {
  border: none;
  background: transparent;
  padding: 5px 11px;
  border-radius: 5px;
  font-size: var(--hm-text-sm);
  color: var(--hm-text-tertiary);
  cursor: pointer;
  transition: background 0.1s, color 0.1s;
}

.layer-switch__item:hover {
  color: var(--hm-text-secondary);
}

.layer-switch__item--active {
  background: var(--hm-bg-overlay);
  color: var(--hm-text-primary);
  font-weight: 500;
  box-shadow: 0 1px 2px rgba(0, 0, 0, 0.25);
}

.zoom-group {
  display: flex;
  align-items: center;
  gap: 1px;
  padding: 2px;
  border-radius: 7px;
  border: 0.5px solid var(--hm-border-subtle);
  background: var(--hm-bg-elevated);
}

.zoom-group__btn {
  display: flex;
  align-items: center;
  justify-content: center;
  width: 24px;
  height: 24px;
  border: none;
  border-radius: 5px;
  background: transparent;
  color: var(--hm-text-secondary);
  cursor: pointer;
}

.zoom-group__btn:hover {
  background: var(--hm-bg-overlay);
  color: var(--hm-text-primary);
}

.zoom-group__label {
  font-size: 10px;
  color: var(--hm-text-tertiary);
  padding: 0 4px;
  min-width: 22px;
  text-align: center;
}

.suggest-btn {
  display: flex;
  align-items: center;
  gap: 6px;
  background: var(--hm-accent);
  border-color: var(--hm-accent);
  color: var(--hm-bg-base);
  font-weight: 600;
}

.suggest-btn:hover {
  background: color-mix(in srgb, var(--hm-accent) 88%, white);
}
</style>

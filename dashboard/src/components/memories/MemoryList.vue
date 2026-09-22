<script setup>
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { PhMagnifyingGlass, PhPlus } from '@phosphor-icons/vue'
import { useMemoriesStore } from '../../stores/memories.js'
import { useUiStore } from '../../stores/ui.js'
import MemoryCard from './MemoryCard.vue'
import FilterChip from '../shared/FilterChip.vue'
import SkeletonCard from '../shared/SkeletonCard.vue'
import TagFilter from '../shared/TagFilter.vue'

const memories = useMemoriesStore()
const ui = useUiStore()
const searchEl = ref(null)

function handleSlash(e) {
  if (e.key !== '/' || e.ctrlKey || e.metaKey || e.altKey) return
  const tag = document.activeElement?.tagName
  if (tag === 'INPUT' || tag === 'TEXTAREA') return
  e.preventDefault()
  searchEl.value?.focus()
}

onMounted(() => window.addEventListener('keydown', handleSlash))
onBeforeUnmount(() => window.removeEventListener('keydown', handleSlash))

const ALL_FILTERS = [
  { label: 'all', value: 'all' },
  { label: 'personal', value: 'personal', layer: 'personal' },
  { label: 'workspace', value: 'workspace', layer: 'workspace' },
  { label: 'org', value: 'org', layer: 'org' },
]

// Org filter only makes sense when the server actually has [org_sync]
// configured — otherwise it's a picker for a layer nothing can ever be in.
const filters = computed(() =>
  ui.orgInfo?.configured ? ALL_FILTERS : ALL_FILTERS.filter(f => f.value !== 'org')
)
</script>

<template>
  <div class="flex flex-col h-full shrink-0"
    style="width:clamp(240px, 26vw, 320px); border-right:0.5px solid var(--hm-border-subtle)">

    <!-- Header -->
    <div class="px-4 pt-4 pb-3" style="border-bottom:0.5px solid var(--hm-border-subtle)">
      <div class="flex items-center gap-2 mb-3">
        <div class="relative flex-1">
          <PhMagnifyingGlass :size="13" class="absolute left-2.5 top-1/2 -translate-y-1/2"
            style="color:var(--hm-text-tertiary)" />
          <input
            ref="searchEl"
            class="hm-input pl-7"
            placeholder="Search…  ( / )"
            :value="memories.searchQuery"
            @input="memories.searchQuery = $event.target.value"
          />
        </div>
        <button class="hm-btn hm-btn-primary hm-btn-sm shrink-0 gap-1" title="New memory"
          @click="memories.startNew()">
          <PhPlus :size="13" weight="bold" />
          New
          <span v-if="memories.hasNewDraft" class="font-mono rounded-sm px-1"
            style="font-size:9px; background:var(--hm-warning-bg); color:var(--hm-warning)">DRAFT</span>
        </button>
      </div>
      <div class="flex items-center flex-wrap gap-y-2 gap-x-2">
        <div class="flex gap-1.5">
          <FilterChip
            v-for="f in filters" :key="f.value"
            :label="f.label" :value="f.value"
            :active="memories.layerFilter === f.value"
            :layer="f.layer"
            @select="memories.layerFilter = $event"
          />
        </div>
        <TagFilter v-model="memories.tagFilter" />
      </div>
    </div>

    <!-- List -->
    <div class="flex-1 overflow-y-auto flex flex-col gap-2 p-2">
      <template v-if="memories.loading">
        <SkeletonCard v-for="i in 5" :key="i" />
      </template>
      <template v-else>
        <MemoryCard
          v-for="mem in memories.filtered"
          :key="mem.id"
          :mem="mem"
          :selected="memories.selected?.id === mem.id"
          @select="memories.select($event)"
        />
        <div v-if="!memories.filtered.length" class="p-6 text-center"
          style="font-size:12px; color:var(--hm-text-secondary)">
          No memories match your filter.
        </div>
      </template>
    </div>

    <!-- Footer -->
    <div class="px-4 flex items-center" style="height:40px; border-top:0.5px solid var(--hm-border-subtle)">
      <span class="font-mono" style="font-size:11px; color:var(--hm-text-tertiary)">
        <template v-if="memories.searchQuery || memories.layerFilter !== 'all' || memories.tagFilter">
          {{ memories.filtered.length }} of {{ memories.all.length }} memories
        </template>
        <template v-else>{{ memories.all.length }} memories</template>
      </span>
    </div>
  </div>
</template>

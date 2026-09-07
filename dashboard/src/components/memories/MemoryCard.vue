<script setup>
import { computed, ref } from 'vue'
import LayerBadge from '../shared/LayerBadge.vue'
import TagChip from '../shared/TagChip.vue'
import Tooltip from '../shared/Tooltip.vue'
import { useMemoriesStore } from '../../stores/memories.js'
import { fmtDate } from '../../lib/format.js'

const memories = useMemoriesStore()
const props = defineProps({ mem: Object, selected: Boolean })
defineEmits(['select'])

const titleTooltip = ref({ visible: false, x: 0, y: 0 })

function onTitleEnter(e) {
  const el = e.currentTarget
  if (el.scrollWidth <= el.clientWidth) return
  const rect = el.getBoundingClientRect()
  titleTooltip.value = { visible: true, x: rect.left + rect.width / 2, y: rect.top }
}

function onTitleLeave() {
  titleTooltip.value.visible = false
}

// project:* is the single-value namespace that identifies what the memory
// belongs to — surface it first regardless of storage order.
const displayTags = computed(() => {
  const tags = props.mem.tags || []
  const projectTag = tags.find(t => t.toLowerCase().startsWith('project:'))
  if (!projectTag) return tags.slice(0, 3)
  return [projectTag, ...tags.filter(t => t !== projectTag)].slice(0, 3)
})
</script>

<template>
  <div
    @click="$emit('select', mem)"
    tabindex="0"
    role="button"
    class="memory-card"
    :class="{
      'memory-card--selected-personal': selected && mem.layer === 'personal',
      'memory-card--selected-org': selected && mem.layer === 'org',
      'memory-card--selected-workspace': selected && mem.layer !== 'personal' && mem.layer !== 'org',
    }"
    @keydown.enter.space.prevent="$emit('select', mem)"
  >
    <!-- Row 1: title + layer badge -->
    <div class="flex items-start justify-between gap-2 mb-1.5">
      <span class="font-medium leading-snug"
        style="font-size:13px; color:var(--hm-text-primary); overflow:hidden; display:-webkit-box; -webkit-line-clamp:1; -webkit-box-orient:vertical"
        @mouseenter="onTitleEnter"
        @mouseleave="onTitleLeave">
        <span v-if="memories.isDraft(mem.id)" class="draft-label">DRAFT</span>
        {{ mem.title }}
      </span>
      <LayerBadge :layer="mem.layer" class="shrink-0 mt-0.5" />
    </div>
    <!-- Row 2: snippet -->
    <p class="mb-2"
      style="font-size:12px; line-height:1.5; color:var(--hm-text-secondary); overflow:hidden; display:-webkit-box; -webkit-line-clamp:2; -webkit-box-orient:vertical">
      {{ mem.content }}
    </p>
    <!-- Row 3: tags + date -->
    <div class="flex items-center gap-1 justify-between">
      <div class="flex items-center gap-1 overflow-hidden">
        <TagChip v-for="tag in displayTags" :key="tag" :tag="tag" />
      </div>
      <span class="font-mono shrink-0" style="font-size:11px; color:var(--hm-text-tertiary)">
        {{ fmtDate(mem.updated_at || mem.created_at) }}
      </span>
    </div>
  </div>
  <Tooltip :visible="titleTooltip.visible" :text="mem.title" :x="titleTooltip.x" :y="titleTooltip.y" />
</template>

<style scoped>
.memory-card {
  padding: 12px 13px;
  cursor: pointer;
  border: 0.5px solid var(--hm-border-subtle);
  border-radius: 6px;
  background: var(--hm-bg-surface);
  transition: background 0.1s, border-color 0.1s, box-shadow 0.1s;
}

.memory-card:hover,
.memory-card:focus-visible {
  background: var(--hm-bg-elevated);
  border-color: var(--hm-border-default);
  outline: none;
}

.memory-card:focus-visible {
  outline: 2px solid var(--hm-accent);
  outline-offset: -2px;
}

.memory-card--selected-personal {
  background: var(--hm-personal-bg);
  border-color: var(--hm-personal-dim);
  box-shadow: inset 2px 0 0 var(--hm-personal);
}

.memory-card--selected-workspace {
  background: var(--hm-workspace-bg);
  border-color: var(--hm-workspace-dim);
  box-shadow: inset 2px 0 0 var(--hm-workspace);
}

.memory-card--selected-org {
  background: var(--hm-org-bg);
  border-color: var(--hm-org-dim);
  box-shadow: inset 2px 0 0 var(--hm-org);
}

.memory-card--selected-personal:hover,
.memory-card--selected-personal:focus-visible {
  background: var(--hm-personal-bg);
  border-color: var(--hm-personal-dim);
}

.memory-card--selected-workspace:hover,
.memory-card--selected-workspace:focus-visible {
  background: var(--hm-workspace-bg);
  border-color: var(--hm-workspace-dim);
}

.memory-card--selected-org:hover,
.memory-card--selected-org:focus-visible {
  background: var(--hm-org-bg);
  border-color: var(--hm-org-dim);
}

.draft-label {
  font-family: var(--hm-font-mono);
  font-size: 9px;
  font-weight: 600;
  letter-spacing: 0.03em;
  color: var(--hm-warning);
  margin-right: 5px;
}
</style>

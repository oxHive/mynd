<script setup>
import { ref, computed, watch } from 'vue'
import { getProjectTree } from '../../api/memories.js'
import { useMemoriesStore } from '../../stores/memories.js'
import { buildOutline } from '../../lib/tree.js'
import TreeBranch from './TreeBranch.vue'

const props = defineProps({
  project: { type: String, required: true },
  layer: { type: String, default: 'workspace' },
})
const memories = useMemoriesStore()
const tree = ref(null)
const error = ref('')

async function load() {
  error.value = ''
  try {
    tree.value = await getProjectTree(props.project, props.layer)
  } catch (e) {
    tree.value = null
    error.value = e.detail || e.message
  }
}
// memories.all is replaced on every refresh (including SSE-driven ones), so
// watching it keeps the outline current after any change.
watch(() => [props.project, props.layer, memories.all], load, { immediate: true })

const outline = computed(() => (tree.value ? buildOutline(tree.value) : null))

function open(id) {
  const entry = memories.all.find(m => m.id === id)
  if (entry) memories.select(entry)
}
</script>

<template>
  <div class="flex flex-col p-2" style="font-size:12px">
    <div v-if="error" class="p-4" style="color:var(--hm-danger)">{{ error }}</div>
    <template v-else-if="outline">
      <div v-if="!outline.root" class="p-4 text-center" style="color:var(--hm-text-secondary)">
        No root index for {{ props.project }} yet. Run /memory-audit to build one.
      </div>
      <TreeBranch v-else :node="outline.root" :children="outline.children"
        :selected-id="memories.selected?.id ?? null" @open="open" />
      <div v-if="outline.unhomed.length" class="mt-3 pt-2" style="border-top:0.5px solid var(--hm-border-subtle)">
        <div class="font-mono pb-1" style="font-size:10px; color:var(--hm-text-tertiary)">Unhomed</div>
        <button v-for="n in outline.unhomed" :key="n.id" class="flex justify-between w-full text-left py-1"
          @click="open(n.id)">
          <span style="color:var(--hm-text-primary)">{{ n.title }}</span>
          <span style="font-size:11px; color:var(--hm-warning)">
            {{ n.reachable ? `filed under ${n.homes.length} indexes` : 'no home' }}
          </span>
        </button>
      </div>
    </template>
  </div>
</template>

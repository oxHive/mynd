<script setup>
import { computed, ref, onMounted, onUnmounted } from 'vue'
import { PhX } from '@phosphor-icons/vue'
import { useMemoriesStore } from '../../stores/memories.js'
import { useTagSettingsStore } from '../../stores/tagSettings.js'
import TagChip from './TagChip.vue'

// modelValue is a single filter string: '' (off), 'ns:*' (any value in
// namespace ns), 'ns:value' (exact namespace tag), or a bare tag with no
// namespace. One string keeps the parent's state trivial to wire up and
// matches how tags already round-trip through the rest of the dashboard.
const props = defineProps({ modelValue: { type: String, default: '' } })
const emit = defineEmits(['update:modelValue'])

const memories = useMemoriesStore()
const tagSettings = useTagSettingsStore()
const open = ref(false)
const query = ref('')
const root = ref(null)

function handleDocClick(e) {
  if (open.value && root.value && !root.value.contains(e.target)) open.value = false
}

onMounted(() => document.addEventListener('mousedown', handleDocClick))
onUnmounted(() => document.removeEventListener('mousedown', handleDocClick))

// Built from tags actually in use, not just the registered namespace list —
// a filter menu should offer what's on your memories today, not an aspirational
// schema. Registered namespaces still supply color/order for the ones in use.
const groups = computed(() => {
  const byNs = new Map()
  const bare = new Set()
  for (const m of memories.all) {
    for (const raw of m.tags || []) {
      const tag = raw.toLowerCase()
      const idx = tag.indexOf(':')
      if (idx === -1) {
        bare.add(tag)
        continue
      }
      const ns = tag.slice(0, idx)
      const value = tag.slice(idx + 1)
      if (!byNs.has(ns)) byNs.set(ns, new Set())
      byNs.get(ns).add(value)
    }
  }
  const namespaceOrder = Object.keys(tagSettings.namespaces)
  const nsNames = [...byNs.keys()].sort((a, b) => {
    const ia = namespaceOrder.indexOf(a)
    const ib = namespaceOrder.indexOf(b)
    if (ia !== -1 && ib !== -1) return ia - ib
    if (ia !== -1) return -1
    if (ib !== -1) return 1
    return a.localeCompare(b)
  })
  return {
    namespaces: nsNames.map(ns => ({ ns, values: [...byNs.get(ns)].sort() })),
    bare: [...bare].sort(),
  }
})

const filteredGroups = computed(() => {
  const q = query.value.trim().toLowerCase()
  if (!q) return groups.value
  return {
    namespaces: groups.value.namespaces
      .map(g => ({ ns: g.ns, values: g.ns.includes(q) ? g.values : g.values.filter(v => v.includes(q)) }))
      .filter(g => g.ns.includes(q) || g.values.length),
    bare: groups.value.bare.filter(t => t.includes(q)),
  }
})

const activeLabel = computed(() => props.modelValue || 'Filter by tag')

function select(value) {
  emit('update:modelValue', value)
  open.value = false
  query.value = ''
}

function clear() {
  emit('update:modelValue', '')
}

function toggle() {
  open.value = !open.value
  if (open.value) query.value = ''
}
</script>

<template>
  <div class="relative" ref="root">
    <button class="hm-btn hm-btn-sm" :class="modelValue ? 'hm-btn-default' : 'hm-btn-ghost'"
      style="font-family:var(--hm-font-mono); max-width:100%" @click="toggle">
      <span style="overflow:hidden; text-overflow:ellipsis; white-space:nowrap">{{ activeLabel }}</span>
      <span v-if="modelValue" class="tag-filter-clear inline-flex items-center" @click.stop="clear">
        <PhX :size="11" weight="bold" />
      </span>
    </button>

    <div v-if="open" class="tag-filter-menu">
      <input class="hm-input mb-2" style="font-size:12px" v-model="query" placeholder="Search tags…" autofocus />

      <div v-if="!filteredGroups.namespaces.length && !filteredGroups.bare.length"
        style="font-size:11px; color:var(--hm-text-tertiary); padding:6px 2px">
        No tags match.
      </div>

      <div v-for="g in filteredGroups.namespaces" :key="g.ns" class="mb-2">
        <button class="tag-filter-row" @click="select(`${g.ns}:*`)">
          <TagChip :tag="`${g.ns}:*`" />
          <span style="font-size:10px; color:var(--hm-text-tertiary)">any value</span>
        </button>
        <button v-for="v in g.values" :key="v" class="tag-filter-row" @click="select(`${g.ns}:${v}`)">
          <TagChip :tag="`${g.ns}:${v}`" />
        </button>
      </div>

      <div v-if="filteredGroups.bare.length">
        <button v-for="t in filteredGroups.bare" :key="t" class="tag-filter-row" @click="select(t)">
          <TagChip :tag="t" />
        </button>
      </div>
    </div>
  </div>
</template>

<style scoped>
.tag-filter-clear {
  color: var(--hm-text-tertiary);
  line-height: 1;
  padding: 0 1px;
}

.tag-filter-clear:hover {
  color: var(--hm-text-primary);
}

.tag-filter-menu {
  position: absolute;
  top: calc(100% + 4px);
  left: 0;
  z-index: 20;
  width: 240px;
  max-height: 340px;
  overflow-y: auto;
  padding: 8px;
  border-radius: 8px;
  border: 0.5px solid var(--hm-border-default);
  background: var(--hm-bg-overlay);
  box-shadow: 0 8px 24px color-mix(in srgb, var(--hm-bg-base) 70%, black);
}

.tag-filter-row {
  display: flex;
  align-items: center;
  gap: 6px;
  width: 100%;
  text-align: left;
  padding: 4px 4px;
  border-radius: 5px;
  background: transparent;
  border: none;
  cursor: pointer;
}

.tag-filter-row:hover {
  background: var(--hm-bg-elevated);
}
</style>

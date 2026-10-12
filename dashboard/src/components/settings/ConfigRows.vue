<script setup>
import { computed } from 'vue'
import { useConfigFileStore } from '../../stores/configFile.js'

// Editor for an array of tables such as [[matrix.rooms]]: one row per
// table with an id column, an optional alias and comma-separated base tags.
const props = defineProps({
  section: { type: String, required: true },
  k: { type: String, required: true },
  idKey: { type: String, required: true },
  idLabel: { type: String, required: true },
  idPlaceholder: String,
})

const config = useConfigFileStore()
const rows = computed(() => config.get(props.section, props.k) ?? [])
const locked = computed(() => config.locked)

function update(i, field, value) {
  const next = rows.value.map(r => ({ ...r }))
  if (field === 'base_tags') value = value.split(',').map(s => s.trim()).filter(Boolean)
  if (value === '' || (Array.isArray(value) && !value.length)) delete next[i][field]
  else next[i][field] = value
  config.set(props.section, props.k, next)
}
const add = () => config.set(props.section, props.k, [...rows.value, { [props.idKey]: '' }])
const remove = i => config.set(props.section, props.k, rows.value.filter((_, j) => j !== i))

const cell = 'hm-input h-auto! px-[9px] py-1.5 text-[12.5px]'
</script>

<template>
  <div class="flex flex-col gap-2 py-3.5" :title="locked ? 'Locked by [dashboard] allow_config_edit = false in config.toml' : undefined">
    <div v-if="rows.length" class="grid grid-cols-[minmax(0,1.4fr)_minmax(0,1fr)_minmax(0,1.4fr)_auto] items-center gap-1.5">
      <span class="hm-label mb-0">{{ idLabel }}</span><span class="hm-label mb-0">Alias</span><span class="hm-label mb-0">Base tags</span><span></span>
      <template v-for="(r, i) in rows" :key="i">
        <input :value="r[idKey]" :placeholder="idPlaceholder" :aria-label="idLabel" :disabled="locked" :class="[cell, 'font-headline']" @change="update(i, idKey, $event.target.value.trim())" />
        <input :value="r.alias ?? ''" placeholder="optional" aria-label="Alias" :disabled="locked" :class="cell" @change="update(i, 'alias', $event.target.value.trim())" />
        <input :value="(r.base_tags ?? []).join(', ')" placeholder="project:x, topic:y" aria-label="Base tags" :disabled="locked" :class="[cell, 'font-headline']" @change="update(i, 'base_tags', $event.target.value)" />
        <button type="button" :aria-label="'Remove ' + (r[idKey] || 'row')" :disabled="locked" class="inline-flex h-7 w-7 items-center justify-center rounded-lg text-fg-faint hover:bg-danger-soft hover:text-danger disabled:pointer-events-none disabled:opacity-50" @click="remove(i)">
          <span class="material-symbols-outlined text-[17px]!" aria-hidden="true">delete</span>
        </button>
      </template>
    </div>
    <div>
      <button type="button" class="hm-btn hm-btn-default hm-btn-sm" :disabled="locked" @click="add">
        <span class="material-symbols-outlined text-[18px]!" aria-hidden="true">add</span>Add {{ idLabel.toLowerCase() }}
      </button>
    </div>
  </div>
</template>

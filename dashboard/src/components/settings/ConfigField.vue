<script setup>
import { computed } from 'vue'
import { useConfigFileStore } from '../../stores/configFile.js'
import SettingRow from './SettingRow.vue'
import Switch from '../shared/Switch.vue'

const props = defineProps({
  section: { type: String, required: true },
  k: { type: String, required: true },
  label: { type: String, required: true },
  hint: String,
  type: { type: String, default: 'text' }, // text | number | bool | list | select
  // Shown when the file doesn't set the key: mynd's default.
  placeholder: [String, Number, Boolean],
  unit: String,
  options: Array, // select: [{ value, label }]
  // A reason string makes the field read-only regardless of the lock.
  readonly: String,
})

const config = useConfigFileStore()
const LOCKED = 'Locked by [dashboard] allow_config_edit = false in config.toml'
const reason = computed(() => props.readonly || (config.locked ? LOCKED : ''))
const id = `cfg-${props.section}-${props.k}`

const model = computed({
  get: () => {
    const v = config.get(props.section, props.k)
    if (props.type === 'bool') return v ?? props.placeholder ?? false
    if (props.type === 'list') return (v ?? []).join(', ')
    return v ?? ''
  },
  set: v => {
    if (props.type === 'list') v = v.split(',').map(s => s.trim()).filter(Boolean)
    config.set(props.section, props.k, v)
  },
})
</script>

<template>
  <SettingRow :label="label" :hint="hint">
    <div class="flex items-center gap-[9px]" :class="reason ? '[&_:disabled]:pointer-events-none' : ''" :title="reason || undefined">
      <Switch v-if="type === 'bool'" v-model="model" :label="label" :disabled="!!reason" />
      <select v-else-if="type === 'select'" :id="id" v-model="model" :aria-label="label" :disabled="!!reason" class="hm-input w-auto">
        <option value="">{{ placeholder }}</option>
        <option v-for="o in options" :key="o.value" :value="o.value">{{ o.label }}</option>
      </select>
      <input v-else-if="type === 'number'" :id="id" v-model.number="model" type="number" min="0" :aria-label="label" :placeholder="String(placeholder ?? '')" :disabled="!!reason"
        class="hm-input w-[130px] font-headline tabular-nums" />
      <input v-else :id="id" v-model.lazy="model" :aria-label="label" :placeholder="String(placeholder ?? '')" :disabled="!!reason" class="hm-input flex-1" />
      <span v-if="unit" class="whitespace-nowrap text-[13px] text-fg-faint">{{ unit }}</span>
      <span v-if="readonly" class="material-symbols-outlined text-[17px]! text-fg-faint" aria-hidden="true">lock</span>
    </div>
  </SettingRow>
</template>

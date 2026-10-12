<script>
export const TNS_COLORS = ['#4a9eff', '#e0607e', '#5fb8b0', '#a875d1', '#1d9e75', '#7c7ce0', '#b8740f', '#e05050']
</script>

<script setup>
import { computed, ref } from 'vue'
import { useTagSettingsStore } from '../../stores/tagSettings.js'
import Switch from '../shared/Switch.vue'
import SegmentedControl from '../shared/SegmentedControl.vue'

const props = defineProps({ name: { type: String, required: true }, open: Boolean })
const emit = defineEmits(['toggle', 'remove'])

const HEX_RE = /^#[0-9a-fA-F]{3,8}$/
const MODE_OPTIONS = [
  { value: 'suggestion', label: 'Suggested', title: 'The listed values show up as autocomplete suggestions, but any typed value is still accepted.' },
  { value: 'fixed', label: 'Fixed', title: 'Only the listed values are accepted. Tagging with anything else is rejected, for AI agents too.' },
]

const tagSettings = useTagSettingsStore()
const ns = computed(() => tagSettings.namespaces[props.name])
// Predefined namespaces are fully read-only while the guard is active (the
// API rejects any change to them). Disable it via [tags]
// guard_predefined_namespaces = false in the global mynd config.
const ro = computed(() => tagSettings.isPredefined(props.name) && tagSettings.guardPredefinedNamespaces)
const n = computed(() => ns.value.values.length)
const mode = computed({ get: () => ns.value.values_mode || 'suggestion', set: v => { ns.value.values_mode = v } })
const draft = ref('')
const confirmRemove = ref(false)

function addValue() {
  const v = draft.value.trim().toLowerCase().replace(/\s+/g, '-').replace(/^.*:/, '')
  if (v && !ns.value.values.includes(v)) ns.value.values.push(v)
  draft.value = ''
}
function removeValue(v) {
  ns.value.values = ns.value.values.filter(x => x !== v)
}
function onKey(e) {
  if (e.key === 'Enter' || e.key === ',') { e.preventDefault(); addValue() }
}

const warn = computed(() => mode.value === 'fixed' && !n.value && !ro.value)
const hint = computed(() => {
  if (!n.value) return warn.value ? 'Fixed with no values enforces nothing yet. Add at least one value to start restricting this namespace.' : 'Any value is accepted.'
  return mode.value === 'fixed' ? 'Only these values are accepted, enforced everywhere including for AI agents.' : 'Offered as autocomplete when tagging. Any value is still accepted.'
})
</script>

<template>
  <div :class="open ? 'bg-panel' : ''">
    <button
      type="button"
      :aria-expanded="open"
      class="flex w-full items-center gap-2.5 py-3 pl-[18px] pr-3.5 text-left transition-colors duration-150"
      :class="open ? '' : 'hover:bg-card-hover'"
      @click="emit('toggle')"
    >
      <span class="h-2.5 w-2.5 shrink-0 rounded-circle" :style="{ background: ns.color }"></span>
      <span class="font-headline text-sm font-semibold text-fg-strong">{{ name }}<span class="font-normal text-fg-faint">:*</span></span>
      <span v-if="ns.single_value" class="inline-flex text-fg-faint" title="One per memory"><span class="material-symbols-outlined text-[17px]!" aria-hidden="true">looks_one</span></span>
      <span class="ml-auto inline-flex items-center gap-2 whitespace-nowrap text-xs text-fg-muted">
        <template v-if="n">
          <span
            class="hidden rounded border px-[7px] py-0.5 font-headline text-[10px] font-semibold uppercase tracking-[0.12em] @min-[520px]:inline"
            :class="mode === 'fixed' ? 'border-fg-strong bg-fg-strong text-card' : 'border-line-strong text-fg-muted'"
          >{{ mode === 'fixed' ? 'Fixed' : 'Suggested' }}</span>
          <span>{{ n + ' value' + (n === 1 ? '' : 's') }}</span>
        </template>
        <span v-else>Any value</span>
      </span>
      <span class="material-symbols-outlined text-[20px]! text-fg-faint transition-transform duration-200 ease-out" :class="open ? 'rotate-180' : ''" aria-hidden="true">expand_more</span>
    </button>

    <div v-if="open" class="flex animate-fade flex-col gap-[18px] px-[18px] pb-4 pt-1 @min-[520px]:pl-[38px]">
      <div class="flex flex-col gap-2">
        <div class="hm-label mb-0">Description</div>
        <p v-if="ro" class="m-0 max-w-[62ch] text-pretty text-[13.5px] leading-[1.6] text-fg">{{ ns.description || 'No description.' }}</p>
        <textarea
          v-else
          v-model="ns.description"
          rows="2"
          class="hm-input h-auto! min-h-[60px] resize-y py-[9px] text-[13.5px] leading-normal"
          placeholder="What does this namespace mean? Shown to AI agents and in this UI."
          aria-label="Description"
        ></textarea>
      </div>

      <div class="flex flex-col gap-2">
        <div class="hm-label mb-0">Color</div>
        <div v-if="ro" class="flex items-center gap-2 text-[12.5px] text-fg-muted">
          <span class="h-2.5 w-2.5 rounded-circle" :style="{ background: ns.color }"></span><span class="font-headline">{{ ns.color }}</span>
        </div>
        <div v-else class="flex flex-wrap items-center gap-2">
          <button
            v-for="c in TNS_COLORS"
            :key="c"
            type="button"
            :title="c"
            :aria-label="'Color ' + c"
            :aria-pressed="ns.color.toLowerCase() === c"
            class="h-[22px] w-[22px] rounded-circle p-0 transition-shadow duration-150"
            :class="ns.color.toLowerCase() === c ? 'shadow-[0_0_0_2px_var(--panel),0_0_0_4px_var(--c)]' : 'shadow-[inset_0_0_0_1px_rgba(11,15,16,.12)]'"
            :style="{ background: c, '--c': c }"
            @click="ns.color = c"
          ></button>
          <input
            v-model="ns.color"
            maxlength="9"
            aria-label="Hex color"
            class="hm-input ml-1 h-auto! w-24 px-[9px] py-1.5 font-headline text-[12.5px]"
            :class="HEX_RE.test(ns.color) ? '' : 'border-danger!'"
          />
        </div>
      </div>

      <label class="flex items-center gap-2.5 text-[13.5px] text-fg" :class="ro ? 'cursor-not-allowed' : 'cursor-pointer'">
        <Switch v-model="ns.single_value" :disabled="ro" :label="`Only one ${name}:* tag per memory`" />
        <span>Only one <span class="font-headline">{{ name }}:*</span> tag per memory</span>
      </label>

      <div class="flex flex-col gap-2">
        <div class="flex items-center justify-between gap-3">
          <span class="hm-label mb-0">Values</span>
          <SegmentedControl v-if="!ro" v-model="mode" :options="MODE_OPTIONS" size="sm" :label="`How ${name} values are enforced`" />
          <span v-else-if="n" class="font-headline text-xs font-semibold text-fg-muted">{{ mode === 'fixed' ? 'Fixed' : 'Suggested' }}</span>
        </div>
        <div class="flex flex-wrap items-center gap-1.5">
          <span
            v-for="v in ns.values"
            :key="v"
            class="inline-flex items-center gap-0.5 rounded border border-[color-mix(in_srgb,var(--c)_38%,transparent)] bg-[color-mix(in_srgb,var(--c)_13%,var(--card))] px-2 py-[3px] font-headline text-xs font-medium leading-[1.3] text-[color-mix(in_srgb,var(--c)_55%,var(--fg-strong))]"
            :style="{ '--c': ns.color }"
          >
            {{ name }}:{{ v }}
            <button v-if="!ro" type="button" :aria-label="'Remove ' + v" class="-mr-[3px] inline-flex opacity-55 hover:opacity-100" @click="removeValue(v)">
              <span class="material-symbols-outlined text-[14px]!" aria-hidden="true">close</span>
            </button>
          </span>
          <div v-if="!ro" class="inline-flex items-center rounded border border-dashed border-line-strong bg-field px-2 transition-colors duration-150 focus-within:border-solid focus-within:border-accent">
            <span class="font-headline text-xs text-fg-faint">{{ name }}:</span>
            <input
              v-model="draft"
              placeholder="add value"
              aria-label="Add value"
              class="w-24 border-0 bg-transparent py-1 font-headline text-xs text-fg outline-none"
              @keydown="onKey"
              @blur="addValue"
            />
          </div>
          <span v-if="ro && !n" class="text-[12.5px] text-fg-faint">No predefined values</span>
        </div>
        <div class="text-xs" :class="warn ? 'text-gold' : 'text-fg-faint'">{{ hint }}</div>
      </div>

      <div v-if="!ro" class="-mt-1 flex items-center justify-end gap-2">
        <template v-if="confirmRemove">
          <span class="mr-1 text-xs text-fg-muted">Remove <span class="font-headline">{{ name }}:*</span> and its values?</span>
          <button type="button" class="hm-btn hm-btn-ghost hm-btn-sm" @click="confirmRemove = false">Cancel</button>
          <button type="button" class="hm-btn hm-btn-danger hm-btn-sm" @click="emit('remove')">Remove</button>
        </template>
        <button v-else type="button" class="hm-btn hm-btn-ghost hm-btn-sm text-danger hover:bg-danger-soft" @click="confirmRemove = true">
          <span class="material-symbols-outlined text-[17px]!" aria-hidden="true">delete</span>Remove namespace
        </button>
      </div>
    </div>
  </div>
</template>

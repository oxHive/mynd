<script setup>
import { ref, onMounted } from 'vue'
import { useTagSettingsStore } from '../../stores/tagSettings.js'
import { useUiStore } from '../../stores/ui.js'
import TagChip from '../shared/TagChip.vue'
import Tooltip from '../shared/Tooltip.vue'

const tagSettings = useTagSettingsStore()
const ui = useUiStore()
const newNamespaceName = ref('')
const newValueInput = ref({})
const saving = ref(false)
const error = ref('')
const pendingDelete = ref(null)
const valuesModeTooltip = ref({ visible: false, x: 0, y: 0, text: '' })

// Predefined namespaces are fully read-only in this UI while the guard is
// active: color, description, single_value, values_mode, and values all
// locked. Disable the guard via [tags] guard_predefined_namespaces = false
// in the global mynd config to edit them here again.
function isLocked(name) {
  return tagSettings.isPredefined(name) && tagSettings.guardPredefinedNamespaces
}

const SWATCHES = ['#4a9eff', '#e0607e', '#5fb8b0', '#a875d1', '#1d9e75', '#7f77dd', '#ba7517', '#d9534f']
const HEX_RE = /^#[0-9a-fA-F]{3,8}$/
const VALUES_MODE_HELP = {
  suggestion: 'The listed values show up as autocomplete suggestions, but any typed value is still accepted.',
  fixed: 'Only the listed values are accepted. Tagging with anything else is rejected, for AI agents too.',
}

function showValuesModeTooltip(e, mode) {
  const rect = e.currentTarget.getBoundingClientRect()
  valuesModeTooltip.value = {
    visible: true, x: rect.left + rect.width / 2, y: rect.top, text: VALUES_MODE_HELP[mode],
  }
}

function hideValuesModeTooltip() {
  valuesModeTooltip.value.visible = false
}

function valuesCaption(ns) {
  const mode = ns.values_mode || 'suggestion'
  if (!ns.values.length) {
    return mode === 'fixed'
      ? 'Fixed, but empty, so nothing is enforced yet. Add a value below to start restricting this namespace.'
      : 'No values yet. Add some below to offer them as autocomplete suggestions when tagging.'
  }
  return mode === 'fixed'
    ? 'Only these values are accepted for this namespace, enforced everywhere, including for AI agents.'
    : 'Shown as autocomplete suggestions when tagging. Any value is still accepted.'
}

onMounted(() => {
  if (!tagSettings.loaded) tagSettings.fetchNamespaces()
})

function setColor(ns, color) {
  tagSettings.namespaces[ns].color = color
}

function addValue(ns) {
  const v = (newValueInput.value[ns] || '').trim().toLowerCase()
  if (v && !tagSettings.namespaces[ns].values.includes(v)) {
    tagSettings.namespaces[ns].values.push(v)
  }
  newValueInput.value[ns] = ''
}

function removeValue(ns, v) {
  tagSettings.namespaces[ns].values = tagSettings.namespaces[ns].values.filter(x => x !== v)
}

function addNamespace() {
  const name = newNamespaceName.value.trim().toLowerCase()
  if (!name || tagSettings.namespaces[name]) return
  tagSettings.namespaces[name] = {
    color: SWATCHES[0], values: [], single_value: false, description: '', values_mode: 'suggestion',
  }
  newNamespaceName.value = ''
}

function askDeleteNamespace(name) {
  if (pendingDelete.value === name) {
    delete tagSettings.namespaces[name]
    pendingDelete.value = null
  } else {
    pendingDelete.value = name
  }
}

async function save() {
  saving.value = true
  error.value = ''
  try {
    await tagSettings.save()
    ui.showToast('Tag namespaces saved')
  } catch {
    error.value = 'Save failed. Check namespace colors and values, then try again.'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <div>
    <p class="hm-label mb-4">TAG NAMESPACES</p>
    <p v-if="!tagSettings.loaded" style="font-size:12px; color:var(--hm-text-tertiary)">Loading…</p>
    <template v-else>
      <p v-if="Object.keys(tagSettings.namespaces).length === 0" class="mb-4"
        style="font-size:12px; color:var(--hm-text-tertiary)">
        No namespaces yet. Tags like <code class="font-mono">topic:idea</code> get a color once you
        define <code class="font-mono">topic</code> below.
      </p>

      <div v-for="(ns, name) in tagSettings.namespaces" :key="name" class="rounded-md mb-3 p-3"
        style="background:var(--hm-bg-elevated); border:0.5px solid var(--hm-border-subtle)">
        <div class="flex items-center gap-2 mb-3">
          <TagChip :tag="`${name}:example`" size="md" />
          <span v-if="tagSettings.isPredefined(name)" class="font-mono rounded-sm px-1.5 py-0.5"
            style="font-size:12px; background:var(--hm-bg-elevated); color:var(--hm-text-tertiary); border:0.5px solid var(--hm-border-subtle)"
            :title="isLocked(name) ? 'Built into Mynd, locked from deletion/edits. Disable via [tags] guard_predefined_namespaces = false in the global config.' : 'Built into Mynd, but the guard is currently disabled, so it is editable/removable.'">
            PREDEFINED
          </span>
          <span class="flex-1"></span>
          <template v-if="!isLocked(name)">
            <button v-if="pendingDelete !== name" class="hm-btn hm-btn-ghost hm-btn-sm"
              style="color:var(--hm-text-tertiary)" @click="askDeleteNamespace(name)">Remove</button>
            <template v-else>
              <span style="font-size:12px; color:var(--hm-text-tertiary)">Remove {{ name }}:*?</span>
              <button class="hm-btn hm-btn-danger hm-btn-sm" @click="askDeleteNamespace(name)">Confirm</button>
              <button class="hm-btn hm-btn-ghost hm-btn-sm" @click="pendingDelete = null">Cancel</button>
            </template>
          </template>
        </div>

        <textarea class="hm-input mb-4 resize-none" style="font-size:12px; height:auto; padding:8px 10px; white-space:pre-wrap"
          rows="2" :disabled="isLocked(name)"
          v-model="ns.description" placeholder="What does this namespace mean? Shown to AI agents and in this UI."></textarea>

        <p class="hm-label" style="margin-bottom:8px">COLOR</p>
        <div v-if="isLocked(name)" class="flex items-center gap-1.5 mb-4">
          <span class="rounded-full" style="width:16px; height:16px; border:1px solid var(--hm-border-default); flex-shrink:0"
            :style="{ background: ns.color }"></span>
          <span class="font-mono" style="font-size:11px; color:var(--hm-text-secondary)">{{ ns.color }}</span>
        </div>
        <div v-else class="flex items-center gap-1.5 mb-4">
          <button v-for="c in SWATCHES" :key="c"
            class="rounded-full"
            style="width:16px; height:16px; border:1px solid var(--hm-border-default); flex-shrink:0"
            :style="{ background: c, outline: ns.color === c ? '2px solid var(--hm-accent)' : 'none', outlineOffset: '1px' }"
            :aria-label="`Use color ${c}`"
            @click="setColor(name, c)"></button>
          <input class="hm-input font-mono" style="width:98px; height:24px; font-size:11px; padding:0 6px"
            v-model="ns.color"
            :style="!HEX_RE.test(ns.color) ? `border-color:var(--hm-danger-border)` : ''" />
        </div>

        <label class="flex items-center gap-2 mb-4" :class="isLocked(name) ? 'cursor-not-allowed' : 'cursor-pointer'">
          <input type="checkbox" v-model="ns.single_value" class="w-3.5 h-3.5" :disabled="isLocked(name)"
            :class="isLocked(name) ? 'cursor-not-allowed' : ''" />
          <span style="font-size:12px; color:var(--hm-text-secondary)">
            Only one {{ name }}:* tag per memory
          </span>
        </label>

        <template v-if="!isLocked(name) || ns.values.length">
          <div class="flex items-center justify-between" style="margin-bottom:8px">
            <p class="hm-label" style="margin-bottom:0">VALUES</p>
            <div v-if="!isLocked(name)" class="seg" role="radiogroup" :aria-label="`How ${name} values are enforced`">
              <button type="button" class="seg-btn" :class="{ 'seg-btn--active': (ns.values_mode || 'suggestion') === 'suggestion' }"
                @click="ns.values_mode = 'suggestion'"
                @mouseenter="showValuesModeTooltip($event, 'suggestion')" @mouseleave="hideValuesModeTooltip">Suggestions</button>
              <button type="button" class="seg-btn" :class="{ 'seg-btn--active': ns.values_mode === 'fixed' }"
                @click="ns.values_mode = 'fixed'"
                @mouseenter="showValuesModeTooltip($event, 'fixed')" @mouseleave="hideValuesModeTooltip">Fixed</button>
            </div>
            <div v-else class="seg">
              <span class="seg-btn seg-btn--active" style="cursor:default">
                {{ (ns.values_mode || 'suggestion') === 'fixed' ? 'Fixed' : 'Suggestions' }}
              </span>
            </div>
          </div>
          <div class="flex flex-wrap items-center gap-1.5 mb-1.5">
            <TagChip v-for="v in ns.values" :key="v" :tag="`${name}:${v}`" :removable="!isLocked(name)" @remove="removeValue(name, v)" />
            <input v-if="!isLocked(name)" class="hm-input" style="width:120px; height:24px; font-size:11px; padding:0 6px"
              v-model="newValueInput[name]" placeholder="Add a value…"
              @keydown.enter="addValue(name)" />
          </div>
          <p style="font-size:11px; color:var(--hm-text-tertiary)">{{ valuesCaption(ns) }}</p>
        </template>
      </div>

      <div class="flex items-center gap-2 mb-4 rounded-md p-3"
        style="border:1px dashed var(--hm-border-default)">
        <input class="hm-input" style="width:160px" v-model="newNamespaceName" placeholder="new namespace"
          @keydown.enter="addNamespace" />
        <button class="hm-btn hm-btn-default hm-btn-sm" @click="addNamespace">+ Add namespace</button>
      </div>

      <div class="flex items-center gap-3">
        <button class="hm-btn hm-btn-primary" :disabled="saving || !tagSettings.isDirty" @click="save">
          {{ saving ? 'Saving…' : 'Save tag namespaces' }}
        </button>
        <span v-if="error" style="font-size:11px; color:var(--hm-danger)">{{ error }}</span>
      </div>
    </template>
    <Tooltip :visible="valuesModeTooltip.visible" :text="valuesModeTooltip.text"
      :x="valuesModeTooltip.x" :y="valuesModeTooltip.y" />
  </div>
</template>

<style scoped>
.seg {
  display: inline-flex;
  border-radius: 5px;
  border: 0.5px solid var(--hm-border-default);
  overflow: hidden;
}

.seg-btn {
  border: none;
  background: transparent;
  color: var(--hm-text-tertiary);
  font-size: 11px;
  font-family: var(--hm-font-mono);
  padding: 5px 10px;
  cursor: pointer;
  transition: background 0.1s, color 0.1s;
}

.seg-btn + .seg-btn {
  border-left: 0.5px solid var(--hm-border-default);
}

.seg-btn:hover {
  color: var(--hm-text-secondary);
}

.seg-btn--active {
  background: var(--hm-bg-overlay);
  color: var(--hm-text-primary);
}

.seg-btn:focus-visible {
  outline: 2px solid var(--hm-accent);
  outline-offset: -2px;
}
</style>

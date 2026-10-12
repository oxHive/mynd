<script setup>
import { ref, computed, onMounted } from 'vue'
import { useTagSettingsStore } from '../../stores/tagSettings.js'
import { useUiStore } from '../../stores/ui.js'
import NamespaceRow, { TNS_COLORS } from './NamespaceRow.vue'

const tagSettings = useTagSettingsStore()
const ui = useUiStore()
const open = ref(null)
const newName = ref('')
const saving = ref(false)
const error = ref('')

onMounted(() => {
  if (!tagSettings.loaded) tagSettings.fetchNamespaces()
})

const names = computed(() => Object.keys(tagSettings.namespaces))
const pre = computed(() => names.value.filter(n => tagSettings.isPredefined(n)))
const custom = computed(() => names.value.filter(n => !tagSettings.isPredefined(n)))
const slug = computed(() => newName.value.trim().toLowerCase().replace(/[^a-z0-9-]/g, ''))
const taken = computed(() => slug.value in tagSettings.namespaces)

const toggle = name => { open.value = open.value === name ? null : name }

function add() {
  if (!slug.value || taken.value) return
  tagSettings.namespaces[slug.value] = {
    color: TNS_COLORS[names.value.length % TNS_COLORS.length],
    values: [], single_value: false, description: '', values_mode: 'suggestion',
  }
  open.value = slug.value
  newName.value = ''
}

function remove(name) {
  delete tagSettings.namespaces[name]
  open.value = null
}

function discard() {
  tagSettings.discard()
  open.value = null
  error.value = ''
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

const groupHead = 'flex items-baseline gap-2.5 px-0.5'
const microLabel = 'font-headline text-[10px] font-semibold uppercase leading-none tracking-[0.18em] text-fg-faint'
</script>

<template>
  <p v-if="!tagSettings.loaded" class="text-[13px] text-fg-faint">Loading…</p>
  <template v-else>
    <div v-if="pre.length" class="flex flex-col gap-2">
      <div :class="groupHead">
        <span :class="microLabel">Predefined</span>
        <span class="text-xs text-fg-faint">{{ tagSettings.guardPredefinedNamespaces ? 'System defaults. Locked.' : 'System defaults. Editable while the guard is off.' }}</span>
      </div>
      <div class="divide-y divide-line overflow-hidden rounded-xl border border-line bg-card">
        <NamespaceRow v-for="name in pre" :key="name" :name="name" :open="open === name" @toggle="toggle(name)" @remove="remove(name)" />
      </div>
    </div>

    <div class="flex flex-col gap-2">
      <div :class="groupHead">
        <span :class="microLabel">Custom</span>
        <span class="text-xs text-fg-faint">{{ custom.length }} namespace{{ custom.length === 1 ? '' : 's' }}</span>
      </div>
      <div class="divide-y divide-line overflow-hidden rounded-xl border border-line bg-card">
        <NamespaceRow v-for="name in custom" :key="name" :name="name" :open="open === name" @toggle="toggle(name)" @remove="remove(name)" />
        <div class="flex flex-wrap items-center gap-2.5 py-3 pl-[18px] pr-3.5">
          <div
            class="inline-flex items-center gap-0.5 rounded-lg border bg-field px-2.5 transition-[border-color,box-shadow] duration-150 focus-within:border-accent focus-within:shadow-[0_0_0_3px_var(--accent-ring)]"
            :class="taken ? 'border-danger' : 'border-field-line'"
          >
            <input v-model="newName" placeholder="new-namespace" aria-label="New namespace name" class="w-[150px] border-0 bg-transparent py-[7px] font-headline text-[13px] text-fg outline-none" @keydown.enter="add" />
            <span class="font-headline text-fg-faint">:*</span>
          </div>
          <button type="button" class="hm-btn hm-btn-default hm-btn-sm" :disabled="!slug || taken" @click="add">
            <span class="material-symbols-outlined text-[18px]!" aria-hidden="true">add</span>Add namespace
          </button>
          <span v-if="taken" class="text-xs text-danger">Already exists</span>
        </div>
      </div>
    </div>

    <div class="sticky bottom-0 flex items-center justify-end gap-2.5 bg-bg py-3">
      <span v-if="error" class="mr-auto text-[12.5px] text-danger">{{ error }}</span>
      <span v-else class="mr-auto text-[12.5px]" :class="tagSettings.isDirty ? 'font-bold text-gold' : 'text-fg-faint'">{{ tagSettings.isDirty ? 'Unsaved changes' : 'All changes saved' }}</span>
      <button v-if="tagSettings.isDirty" type="button" class="hm-btn hm-btn-ghost" @click="discard">Discard</button>
      <button type="button" class="hm-btn hm-btn-primary" :disabled="saving || !tagSettings.isDirty" @click="save">
        {{ saving ? 'Saving…' : 'Save namespaces' }}
      </button>
    </div>
  </template>
</template>

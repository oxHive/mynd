<script setup>
import { ref } from 'vue'
import { useConfigFileStore } from '../../stores/configFile.js'
import { useUiStore } from '../../stores/ui.js'

// Save / Discard for the shared config.toml draft, or the lock notice.
// Rendered as a bottom bar in ConfigPane and stacked in SettingsRail.
defineProps({ stacked: Boolean })
const config = useConfigFileStore()
const ui = useUiStore()
const saving = ref(false)
const error = ref('')

async function save() {
  saving.value = true
  error.value = ''
  try {
    await config.save()
    ui.showToast('Saved to config.toml')
  } catch (e) {
    error.value = e.detail || 'Save failed.'
  } finally {
    saving.value = false
  }
}

function discard() {
  config.discard()
  error.value = ''
}
</script>

<template>
  <div class="flex items-center gap-2.5" :class="stacked ? 'flex-wrap justify-between' : 'justify-end'">
    <span v-if="config.locked" class="mr-auto inline-flex items-center gap-1.5 text-[12.5px] text-fg-muted">
      <span class="material-symbols-outlined text-[17px]!" aria-hidden="true">lock</span>
      Read-only
    </span>
    <template v-else>
      <span v-if="error" class="mr-auto text-[12.5px] text-danger" :class="stacked ? 'basis-full' : ''">{{ error }}</span>
      <span v-else class="mr-auto text-[12.5px]" :class="config.isDirty ? 'font-bold text-gold' : 'text-fg-faint'">
        {{ config.isDirty ? 'Unsaved changes' : 'All changes saved' }}
      </span>
      <span class="inline-flex gap-2.5" :class="stacked && error ? 'ml-auto' : ''">
        <button v-if="config.isDirty" type="button" class="hm-btn hm-btn-ghost" @click="discard">Discard</button>
        <button type="button" class="hm-btn hm-btn-primary" :disabled="saving || !config.isDirty" @click="save">
          {{ saving ? 'Saving…' : 'Save' }}
        </button>
      </span>
    </template>
  </div>
</template>

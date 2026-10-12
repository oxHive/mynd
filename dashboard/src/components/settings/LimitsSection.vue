<script setup>
import { ref, onMounted } from 'vue'
import { useContentLimitSettingsStore } from '../../stores/contentLimitSettings.js'
import { useUiStore } from '../../stores/ui.js'
import SettingRow from './SettingRow.vue'

const contentLimits = useContentLimitSettingsStore()
const ui = useUiStore()
const saving = ref(false)
const error = ref('')

onMounted(() => {
  if (!contentLimits.loaded) contentLimits.fetch()
})

async function save() {
  saving.value = true
  error.value = ''
  try {
    await contentLimits.save()
    ui.showToast('Content limits saved')
  } catch {
    error.value = 'Save failed. Max content tokens must be a positive number.'
  } finally {
    saving.value = false
  }
}
</script>

<template>
  <p v-if="!contentLimits.loaded" class="text-[13px] text-fg-faint">Loading…</p>
  <template v-else>
    <div class="rounded-xl border border-line bg-card px-[18px] py-1">
      <SettingRow label="Max content tokens" hint="Per memory, title plus content.">
        <div class="flex items-center gap-[9px]">
          <input v-model.number="contentLimits.maxContentTokens" type="number" min="1" aria-label="Max content tokens"
            class="hm-input w-[110px] font-headline tabular-nums" />
          <span class="whitespace-nowrap text-[13px] text-fg-faint">tokens</span>
        </div>
      </SettingRow>
      <hr class="m-0 h-px border-0 bg-line" />
      <p class="m-0 py-3.5 text-[12.5px] leading-[1.6] text-fg-muted">
        Enforced for <span class="font-headline">memory_store</span> and <span class="font-headline">memory_update</span>,
        for AI agents too. Content over the limit is rejected with an error explaining how to split it into an index
        memory plus child memories, so no single memory can dominate a session-start recall.
      </p>
    </div>
    <div class="flex items-center justify-end gap-3">
      <span v-if="error" class="mr-auto text-xs text-danger">{{ error }}</span>
      <button type="button" class="hm-btn hm-btn-primary" :disabled="saving || !contentLimits.isDirty" @click="save">
        {{ saving ? 'Saving…' : 'Save' }}
      </button>
    </div>
  </template>
</template>

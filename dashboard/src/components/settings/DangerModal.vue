<script setup>
import { ref, computed } from 'vue'
import { useMemoriesStore } from '../../stores/memories.js'
import { useUiStore } from '../../stores/ui.js'

defineEmits(['close'])
const memories = useMemoriesStore()
const ui = useUiStore()
const confirmText = ref('')
const working = ref(false)

const canConfirm = computed(() => confirmText.value === 'DELETE')

async function handleClear() {
  working.value = true
  try {
    await memories.clearAll()
    ui.requestActiveView('memories')
    ui.showToast('All memories deleted')
    confirmText.value = ''
  } catch {
    ui.showToast('Failed — server error')
  } finally {
    working.value = false
  }
}
</script>

<template>
  <div class="fixed inset-0 z-40 flex items-center justify-center"
    style="background:rgba(0,0,0,0.6)">
    <div class="rounded-lg p-6 w-96"
      style="background:var(--hm-bg-overlay); border:0.5px solid var(--hm-danger-border)">
      <h3 class="mb-2 font-medium" style="font-size:14px; color:var(--hm-text-primary)">Confirm deletion</h3>
      <p class="mb-4" style="font-size:13px; color:var(--hm-text-secondary)">
        This will permanently delete all {{ memories.all.filter(m => m.layer !== 'org').length }} memories, edges, tags, and feedback.
        This cannot be undone.
      </p>
      <p v-if="memories.all.some(m => m.layer === 'org')" class="mb-4" style="font-size:12px; color:var(--hm-text-tertiary)">
        Org-layer memories are not affected by this action.
      </p>
      <label class="hm-label">TYPE DELETE TO CONFIRM</label>
      <input class="hm-input mb-4" v-model="confirmText" placeholder="DELETE" />
      <div class="flex justify-end gap-2">
        <button class="hm-btn hm-btn-default" @click="$emit('close')">Cancel</button>
        <button class="hm-btn hm-btn-danger" :disabled="!canConfirm || working" @click="handleClear">
          {{ working ? 'Deleting…' : 'Clear all ⚠' }}
        </button>
      </div>
    </div>
  </div>
</template>

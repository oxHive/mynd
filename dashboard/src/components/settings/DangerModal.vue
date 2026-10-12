<script setup>
import { ref, computed } from 'vue'
import { useMemoriesStore } from '../../stores/memories.js'
import { useUiStore } from '../../stores/ui.js'
import Modal from '../shared/Modal.vue'

const emit = defineEmits(['close'])
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
    emit('close')
  } catch {
    ui.showToast('Failed — server error')
  } finally {
    working.value = false
  }
}
</script>

<template>
  <Modal label="Confirm deletion" :persistent="working" @close="emit('close')">
    <h3 class="mb-2 font-headline text-[19px] font-bold tracking-[-0.025em] text-fg-strong">Confirm deletion</h3>
    <p class="mb-5 mt-0 text-sm leading-[1.6] text-fg-muted">
      This will permanently delete all <strong>{{ memories.all.filter(m => m.layer !== 'org').length }}</strong> memories, edges, tags, and feedback. This cannot be undone.
    </p>
    <p v-if="memories.all.some(m => m.layer === 'org')" class="-mt-3 mb-5 text-[12.5px] text-fg-faint">
      Org-layer memories are not affected by this action.
    </p>
    <div class="flex flex-col gap-[7px] py-3">
      <label for="confirm-delete" class="font-headline text-[12.5px] font-semibold text-fg">Type <span class="text-danger">DELETE</span> to confirm:</label>
      <input id="confirm-delete" v-model="confirmText" class="hm-input font-headline" autofocus @keydown.enter="canConfirm && !working && handleClear()" />
    </div>
    <div class="mt-5 flex justify-end gap-2.5">
      <button type="button" class="hm-btn hm-btn-default" :disabled="working" @click="emit('close')">Cancel</button>
      <button type="button" class="hm-btn hm-btn-danger" :disabled="!canConfirm || working" @click="handleClear">
        {{ working ? 'Deleting…' : 'Clear all' }} <span class="material-symbols-outlined text-[16px]!" aria-hidden="true">warning</span>
      </button>
    </div>
  </Modal>
</template>

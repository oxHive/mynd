<script setup>
import { ref, computed } from 'vue'
import { removeTrustedNetwork } from '../../api/hive.js'
import { useHiveStore } from '../../stores/hive.js'
import { useUiStore } from '../../stores/ui.js'
import Modal from '../shared/Modal.vue'

const props = defineProps({ network: { type: Object, required: true } })
const emit = defineEmits(['close'])
const hive = useHiveStore()
const ui = useUiStore()
const confirmText = ref('')
const working = ref(false)
const error = ref('')

// Ids are `ssid:<name>` or `mac:<gateway>`; the user types the bare name.
const expected = computed(() => props.network.id.replace(/^(ssid|mac):/, ''))
const canConfirm = computed(() => confirmText.value === expected.value)

async function handleRemove() {
  working.value = true
  error.value = ''
  try {
    await removeTrustedNetwork(props.network.id)
    await hive.fetchTrustedNetworks()
    ui.showToast(`Removed ${props.network.label || expected.value}`)
    emit('close')
  } catch {
    error.value = 'Could not remove that network — server error.'
  } finally {
    working.value = false
  }
}
</script>

<template>
  <Modal label="Remove trusted network" :persistent="working" @close="emit('close')">
    <h3 class="mb-2 font-headline text-[19px] font-bold tracking-[-0.025em] text-fg-strong">Remove trusted network</h3>
    <p class="mb-5 mt-0 text-sm leading-[1.6] text-fg-muted">
      Hive sync will auto-pause whenever this device is on <strong>{{ network.label || expected }}</strong>.
    </p>
    <div class="flex flex-col gap-[7px] py-3">
      <label for="confirm-network" class="font-headline text-[12.5px] font-semibold text-fg">
        Type <span class="text-danger">{{ expected }}</span> to confirm:
      </label>
      <!-- Typed, not pasted: the point is to read the name. -->
      <input id="confirm-network" v-model="confirmText" class="hm-input font-headline" autocomplete="off" spellcheck="false" autofocus
        @paste.prevent @drop.prevent @keydown.enter="canConfirm && !working && handleRemove()" />
    </div>
    <p v-if="error" class="mb-0 mt-2 text-[12.5px] text-danger">{{ error }}</p>
    <div class="mt-5 flex justify-end gap-2.5">
      <button type="button" class="hm-btn hm-btn-default" :disabled="working" @click="emit('close')">Cancel</button>
      <button type="button" class="hm-btn hm-btn-danger" :disabled="!canConfirm || working" @click="handleRemove">
        {{ working ? 'Removing…' : 'Remove' }}
      </button>
    </div>
  </Modal>
</template>

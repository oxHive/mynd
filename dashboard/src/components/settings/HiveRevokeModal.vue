<script setup>
import { ref } from 'vue'
import { revokeDevice } from '../../api/hive.js'
import { useHiveStore } from '../../stores/hive.js'
import { useUiStore } from '../../stores/ui.js'
import Modal from '../shared/Modal.vue'

const props = defineProps({ deviceId: { type: String, required: true }, deviceName: { type: String, required: true } })
const emit = defineEmits(['close'])
const hive = useHiveStore()
const ui = useUiStore()
const working = ref(false)
const error = ref('')

async function handleRevoke() {
  working.value = true
  error.value = ''
  try {
    await revokeDevice(props.deviceId)
    await hive.fetchStatus()
    ui.showToast(`Revoked ${props.deviceName}`)
    emit('close')
  } catch (e) {
    if (e.status === 409) {
      error.value = 'Revocation was rejected — this device may not be a fully active hive member yet.'
    } else if (e.status === 404) {
      error.value = 'This device is already revoked or no longer in the roster.'
    } else {
      error.value = 'Revoke failed — server error.'
    }
  } finally {
    working.value = false
  }
}
</script>

<template>
  <Modal label="Revoke device" :persistent="working" @close="emit('close')">
    <h3 class="mb-2 font-headline text-[19px] font-bold tracking-[-0.025em] text-fg-strong">Revoke device</h3>
    <p class="mb-5 mt-0 text-sm leading-[1.6] text-fg-muted">
      This will revoke <strong>{{ deviceName }}</strong> from the hive. It will stop syncing
      with this device and every other peer once the revocation propagates. This cannot be undone
      from this device.
    </p>
    <p v-if="error" class="mb-5 text-[12.5px] text-danger">{{ error }}</p>
    <div class="flex justify-end gap-2.5">
      <button type="button" class="hm-btn hm-btn-default" :disabled="working" @click="emit('close')">Cancel</button>
      <button type="button" class="hm-btn hm-btn-danger" :disabled="working" @click="handleRevoke">
        {{ working ? 'Revoking…' : 'Revoke' }}
      </button>
    </div>
  </Modal>
</template>

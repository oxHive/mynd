<script setup>
import { ref } from 'vue'
import { revokeDevice } from '../../api/hive.js'
import { useHiveStore } from '../../stores/hive.js'
import { useUiStore } from '../../stores/ui.js'

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
  <div class="fixed inset-0 z-40 flex items-center justify-center"
    style="background:rgba(0,0,0,0.6)">
    <div class="rounded-lg p-6 w-96"
      style="background:var(--hm-bg-overlay); border:0.5px solid var(--hm-danger-border)">
      <h3 class="mb-2 font-medium" style="font-size:14px; color:var(--hm-text-primary)">Revoke device</h3>
      <p class="mb-4" style="font-size:13px; color:var(--hm-text-secondary)">
        This will revoke <strong>{{ deviceName }}</strong> from the hive. It will stop syncing
        with this device and every other peer once the revocation propagates. This cannot be undone
        from this device.
      </p>
      <p v-if="error" class="mb-4" style="font-size:12px; color:var(--hm-danger)">{{ error }}</p>
      <div class="flex justify-end gap-2">
        <button class="hm-btn hm-btn-default" @click="$emit('close')">Cancel</button>
        <button class="hm-btn hm-btn-danger" :disabled="working" @click="handleRevoke">
          {{ working ? 'Revoking…' : 'Revoke' }}
        </button>
      </div>
    </div>
  </div>
</template>

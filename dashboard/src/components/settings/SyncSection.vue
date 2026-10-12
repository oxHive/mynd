<script setup>
import { ref, onMounted } from 'vue'
import { useUiStore } from '../../stores/ui.js'
import { getSyncSettings, saveSyncSettings } from '../../api/settings.js'
import Switch from '../shared/Switch.vue'
import SettingRow from './SettingRow.vue'

const ui = useUiStore()
const enabled = ref(false)
const remoteUrl = ref('')
const apiKey = ref('')
const intervalSeconds = ref(300)
const syncOnStore = ref(true)
const showKey = ref(false)
const loading = ref(false)
const message = ref('')

onMounted(async () => {
  loading.value = true
  try {
    const s = await getSyncSettings()
    enabled.value = s.enabled ?? false
    remoteUrl.value = s.remote_url ?? ''
    apiKey.value = s.api_key ?? ''
    intervalSeconds.value = s.interval_seconds ?? 300
    syncOnStore.value = s.sync_on_store ?? true
  } catch {
    message.value = 'Could not load sync settings.'
  } finally {
    loading.value = false
  }
})

async function save() {
  const res = await saveSyncSettings({
    enabled: enabled.value, remote_url: remoteUrl.value,
    api_key: apiKey.value, interval_seconds: intervalSeconds.value,
    sync_on_store: syncOnStore.value,
  })
  message.value = res.message ?? 'Edit config.toml to persist sync settings.'
  ui.showToast(message.value)
}

const fieldLabel = 'font-headline text-[12.5px] font-semibold text-fg'
</script>

<template>
  <p v-if="loading" class="text-[13px] text-fg-faint">Loading…</p>
  <template v-else>
    <div class="rounded-xl border border-line bg-card px-[18px] py-1">
      <SettingRow label="Sync enabled"><Switch v-model="enabled" label="Sync enabled" /></SettingRow>
      <div v-if="enabled" class="animate-fade">
        <hr class="m-0 h-px border-0 bg-line" />
        <div class="flex flex-col gap-[7px] py-3">
          <label for="sync-url" :class="fieldLabel">Remote URL</label>
          <input id="sync-url" v-model="remoteUrl" class="hm-input" placeholder="https://your-server.com or http://192.168.1.x:3456" />
        </div>
        <div class="flex flex-col gap-[7px] py-3">
          <label for="sync-key" :class="fieldLabel">API key</label>
          <div class="flex items-center gap-2">
            <input id="sync-key" v-model="apiKey" :type="showKey ? 'text' : 'password'" class="hm-input flex-1 font-headline" placeholder="Leave blank if no auth" />
            <button type="button" :title="showKey ? 'Hide' : 'Show'" :aria-label="showKey ? 'Hide API key' : 'Show API key'"
              class="inline-flex h-8 w-8 shrink-0 items-center justify-center rounded-lg text-fg-muted transition-colors duration-150 hover:bg-chip hover:text-fg"
              @click="showKey = !showKey">
              <span class="material-symbols-outlined text-[19px]!" aria-hidden="true">{{ showKey ? 'visibility_off' : 'visibility' }}</span>
            </button>
          </div>
        </div>
        <div class="flex flex-col gap-[7px] py-3">
          <label for="sync-interval" :class="fieldLabel">Interval</label>
          <div class="flex items-center gap-[9px]">
            <input id="sync-interval" v-model.number="intervalSeconds" type="number" min="30" class="hm-input w-[110px] font-headline tabular-nums" />
            <span class="whitespace-nowrap text-[13px] text-fg-faint">seconds</span>
          </div>
        </div>
        <label class="flex cursor-pointer items-center gap-2.5 pb-3.5 pt-2 text-[13.5px] text-fg-muted">
          <input v-model="syncOnStore" type="checkbox" class="h-4 w-4 accent-[var(--accent)]" />
          Sync immediately after storing a memory
        </label>
      </div>
    </div>
    <div class="flex items-center justify-end gap-3">
      <span v-if="message" class="mr-auto text-xs text-fg-faint">{{ message }}</span>
      <button type="button" class="hm-btn hm-btn-primary" @click="save">Save</button>
    </div>
  </template>
</template>

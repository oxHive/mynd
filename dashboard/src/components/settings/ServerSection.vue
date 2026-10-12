<script setup>
import { computed } from 'vue'
import { useUiStore } from '../../stores/ui.js'
import { useUpdateStore } from '../../stores/update.js'
const ui = useUiStore()
const update = useUpdateStore()

const STATUS_LABEL = { running: 'Running', syncing: 'Syncing', sync_failed: 'Sync failed', unreachable: 'Stopped', checking: 'Checking' }
const ok = computed(() => ui.serverStatus === 'running' || ui.serverStatus === 'syncing')

const info = computed(() => ui.serverInfo ?? {})
const storagePath = computed(() => info.value.db_path || info.value.storage_path || info.value.storagePath || '')
const storageFile = computed(() => storagePath.value.split(/[\\/]/).pop() || '—')

const cell = 'flex min-w-0 flex-col gap-[7px] border-l border-line px-[18px] py-3.5 -ml-px'
const label = 'font-headline text-[10px] font-semibold uppercase leading-none tracking-[0.18em] text-fg-faint'
const val = 'font-headline text-[17px] font-bold leading-[1.2] tracking-[-0.02em] tabular-nums text-fg-strong [overflow-wrap:anywhere]'
const note = 'mt-1 flex items-center gap-0.5 whitespace-nowrap text-[11px] font-semibold tracking-normal'
</script>

<template>
  <div class="grid grid-cols-[repeat(auto-fit,minmax(120px,1fr))] overflow-hidden rounded-xl border border-line bg-card">
    <div :class="cell">
      <div :class="label">Status</div>
      <div :class="val" class="inline-flex items-center gap-2">
        <span class="h-[7px] w-[7px] shrink-0 rounded-circle" :class="ok ? 'bg-ok-dot shadow-[0_0_0_3px_rgba(95,184,122,0.18)]' : 'bg-err-dot'"></span>{{ STATUS_LABEL[ui.serverStatus] ?? ui.serverStatus }}
      </div>
    </div>
    <div :class="cell">
      <div :class="label">Version</div>
      <div :class="val">
        {{ info.version || '—' }}
        <span v-if="update.status === 'updating'" :class="note" class="text-fg-muted">Updating…</span>
        <span v-else-if="update.status === 'failed'" :class="note" class="text-danger"><span class="material-symbols-outlined text-[14px]!" aria-hidden="true">error</span>Update failed</span>
        <button v-else-if="update.available && update.platformSupported" type="button" :class="note" class="text-gold hover:underline" @click="update.changelogOpen = true">
          <span class="material-symbols-outlined text-[14px]!" aria-hidden="true">upgrade</span>v{{ update.latestVersion }} available
        </button>
        <span v-else-if="update.available" :class="note" class="text-gold"><span class="material-symbols-outlined text-[14px]!" aria-hidden="true">upgrade</span>v{{ update.latestVersion }} available</span>
        <span v-else :class="note" class="text-ok"><span class="material-symbols-outlined text-[14px]!" aria-hidden="true">check</span>Up to date</span>
      </div>
    </div>
    <div :class="cell"><div :class="label">Memories</div><div :class="val">{{ info.memory_count ?? info.memoryCount ?? '—' }}</div></div>
    <div :class="cell"><div :class="label">Storage</div><div :class="val" :title="storagePath">{{ storageFile }}</div></div>
    <div :class="cell"><div :class="label">Agent</div><div :class="val">{{ info.agent?.kind || '—' }}</div></div>
  </div>
</template>

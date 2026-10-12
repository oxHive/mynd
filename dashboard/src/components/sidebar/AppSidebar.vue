<script setup>
import { computed } from 'vue'
import { useUiStore } from '../../stores/ui.js'
import { useFeedbackStore } from '../../stores/feedback.js'
import { useGraphStore } from '../../stores/graph.js'
import { useUpdateStore } from '../../stores/update.js'
import logoUrl from '../../assets/oxhive-logo-white.png'
import { BASE } from '../../api/client.js'

const ui = useUiStore()
const feedback = useFeedbackStore()
const graph = useGraphStore()
const update = useUpdateStore()

const NAV_ITEMS = [
  { id: 'memories', label: 'Memories', icon: 'neurology' },
  { id: 'graph', label: 'Graph', icon: 'graph_3' },
  { id: 'analytics', label: 'Analytics', icon: 'insights' },
  { id: 'feedback', label: 'Feedback', icon: 'rate_review' },
  { id: 'settings', label: 'Settings', icon: 'settings' },
]

// Pending connection suggestions: badged on the two pages that surface them
// (both show the review bar), same treatment as the feedback count.
function navBadgeCount(id) {
  if (id === 'feedback') return feedback.conflicts.length + feedback.feedbackItems.length
  if (id === 'memories' || id === 'graph') return graph.pendingEdges.length
  return 0
}

const connected = computed(() => ui.serverStatus !== 'unreachable' && ui.serverStatus !== 'sync_failed')

const memoryCount = computed(() => {
  if (!ui.serverInfo) return '—'
  const primary = ui.serverInfo?.memory_count ?? ui.serverInfo?.memoryCount ?? 0
  const org = ui.orgInfo?.count ?? 0
  return primary + org
})

const serverAddress = computed(() => (BASE || 'http://localhost:3456').replace(/^https?:\/\//, ''))

function syncedLabel(last) {
  if (!last) return 'not yet synced'
  const diffSec = Math.floor(Date.now() / 1000) - last
  if (diffSec < 60) return 'synced just now'
  return `synced ${Math.floor(diffSec / 60)}m ago`
}

const syncStatusText = computed(() =>
  ui.syncInfo?.enabled ? syncedLabel(ui.syncInfo.last_synced_at) : null
)

const orgSyncStatusText = computed(() => {
  if (!ui.orgInfo?.configured) return null
  if (!ui.orgInfo.enabled) return 'sync disabled'
  return syncedLabel(ui.orgInfo.last_synced_at)
})

const conflictCount = computed(() => ui.syncInfo?.conflict_count ?? 0)
</script>

<template>
  <aside class="flex h-full w-[248px] shrink-0 flex-col border-r border-side-border bg-side text-side-fg">
    <!-- Brand -->
    <div class="flex items-center gap-3 border-b border-side-border py-[18px] pl-5 pr-4">
      <img :src="logoUrl" alt="OxHive" class="h-8 w-8 shrink-0" />
      <div class="min-w-0">
        <div class="font-headline text-[9.5px] font-semibold uppercase leading-none tracking-[0.2em] text-primary-dark">OxHive</div>
        <div class="mt-1 font-headline text-xl font-extrabold leading-none tracking-[-0.035em] text-white">Mynd</div>
      </div>
      <span class="ml-auto rounded-full border border-side-border bg-side-hover px-2 py-1 font-headline text-[11px] font-medium leading-none tabular-nums text-side-muted">v{{ ui.serverInfo?.version || '—' }}</span>
    </div>

    <!-- Nav -->
    <nav class="flex flex-col gap-0.5 px-3 py-2">
      <button
        v-for="it in NAV_ITEMS"
        :key="it.id"
        type="button"
        :aria-current="ui.activeView === it.id ? 'page' : undefined"
        class="relative flex w-full items-center gap-3 rounded-[4px] px-3 py-2.5 text-left font-body text-sm font-medium transition-colors duration-150"
        :class="ui.activeView === it.id
          ? 'bg-side-active text-white before:absolute before:-left-3 before:top-1/2 before:h-5 before:w-[3px] before:-translate-y-1/2 before:rounded-r-[2px] before:bg-primary before:content-[\'\'] dark:before:bg-primary-dark'
          : 'text-side-muted hover:bg-side-hover hover:text-side-fg'"
        @click="ui.requestActiveView(it.id)"
      >
        <span class="material-symbols-outlined text-[21px]!" aria-hidden="true">{{ it.icon }}</span>
        <span>{{ it.label }}</span>
        <span
          v-if="navBadgeCount(it.id) > 0"
          class="ml-auto flex h-5 min-w-[20px] items-center justify-center rounded-full bg-gold px-1.5 font-headline text-[11px] font-semibold tabular-nums text-side dark:bg-gold-dark"
        >{{ navBadgeCount(it.id) }}</span>
      </button>
    </nav>

    <div class="flex-1"></div>

    <button
      v-if="update.available && update.platformSupported"
      type="button"
      class="mx-3 flex items-center gap-3 rounded-[4px] px-3 py-2.5 text-left font-body text-sm font-medium text-side-muted transition-colors duration-150 hover:bg-side-hover hover:text-side-fg"
      @click="update.changelogOpen = true"
    >
      <span class="truncate">Update available</span>
      <span class="ml-auto rounded-full bg-gold px-2 py-1 font-headline text-[11px] font-semibold leading-none tabular-nums text-side dark:bg-gold-dark">v{{ update.latestVersion }}</span>
    </button>

    <!-- Status footer -->
    <div class="m-3 flex flex-col rounded-[8px] border border-side-border bg-side-card p-3.5">
      <div class="flex items-center gap-2">
        <span
          class="h-[7px] w-[7px] shrink-0 rounded-full"
          :class="connected ? 'bg-ok-dot shadow-[0_0_0_3px_rgba(95,184,122,0.18)]' : 'bg-err-dot'"
        ></span>
        <span class="font-headline text-[13px] font-semibold text-side-fg">{{ connected ? 'Connected' : 'Disconnected' }}</span>
        <span v-if="connected && syncStatusText" class="ml-auto text-[11px] text-side-muted">{{ syncStatusText }}</span>
      </div>
      <div class="ml-[15px] mt-1 font-headline text-[11.5px] tabular-nums text-side-muted">{{ serverAddress }}</div>
      <div v-if="orgSyncStatusText" class="ml-[15px] mt-1 font-headline text-[11.5px] text-side-muted">org · {{ orgSyncStatusText }}</div>

      <div class="mt-3.5 grid grid-cols-2 gap-px overflow-hidden rounded-[4px] bg-side-border">
        <button
          type="button"
          class="flex flex-col gap-1 bg-side px-2.5 py-[9px] text-left transition-colors duration-150 hover:bg-side-hover"
          @click="ui.requestActiveView('memories')"
        >
          <span class="font-headline text-[9.5px] font-semibold uppercase leading-none tracking-[0.18em] text-side-muted">Memories</span>
          <span class="font-headline text-lg font-bold leading-none tracking-[-0.02em] tabular-nums text-white">{{ memoryCount }}</span>
        </button>
        <div class="flex flex-col gap-1 bg-side px-2.5 py-[9px]">
          <span class="font-headline text-[9.5px] font-semibold uppercase leading-none tracking-[0.18em] text-side-muted">Conflicts</span>
          <span
            class="font-headline text-lg font-bold leading-none tracking-[-0.02em] tabular-nums"
            :class="conflictCount > 0 ? 'text-gold-dark' : 'text-white'"
          >{{ conflictCount }}</span>
        </div>
      </div>
    </div>
  </aside>
</template>

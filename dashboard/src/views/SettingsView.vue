<script setup>
import { ref, computed, onMounted, onBeforeUnmount } from 'vue'
import { useUiStore } from '../stores/ui.js'
import { useTagSettingsStore } from '../stores/tagSettings.js'
import { readStored } from '../lib/localStore.js'
import ServerSection from '../components/settings/ServerSection.vue'
import AppearanceSection from '../components/settings/AppearanceSection.vue'
import SyncSection from '../components/settings/SyncSection.vue'
import HiveSection from '../components/settings/HiveSection.vue'
import TagsSection from '../components/settings/TagsSection.vue'
import LimitsSection from '../components/settings/LimitsSection.vue'
import DataSection from '../components/settings/DataSection.vue'
import DangerSection from '../components/settings/DangerSection.vue'

const ui = useUiStore()
const tagSettings = useTagSettingsStore()

const NAV = [
  { group: 'General', items: [{ id: 'appearance', label: 'Appearance', icon: 'palette' }] },
  { group: 'Connection', items: [{ id: 'sync', label: 'Sync', icon: 'sync' }, { id: 'hive', label: 'Hive', icon: 'hive' }] },
  { group: 'Memory', items: [{ id: 'tags', label: 'Tags', icon: 'sell' }, { id: 'limits', label: 'Limits', icon: 'tune' }] },
  { group: 'Data', items: [{ id: 'data', label: 'Import & export', icon: 'database' }] },
]
const PANES = {
  appearance: ['Appearance', 'Theme and text size. Saved on this device only.', AppearanceSection],
  sync: ['Sync', 'Replicate memories to a remote Mynd server.', SyncSection],
  hive: ['Hive', 'Sync memories peer-to-peer with your other devices.', HiveSection],
  tags: ['Tags', 'Namespaces give tags a namespace:value shape. Descriptions and rules are shown to AI agents.', TagsSection],
  limits: ['Limits', 'Caps on memory length.', LimitsSection],
  data: ['Import & export', 'Back up or restore personal and workspace memories as JSON.', DataSection],
  danger: ['Danger zone', 'Irreversible actions.', DangerSection],
}

const TAB_KEY = 'mynd.settingsTab'
const stored = readStored(TAB_KEY)
const activeTab = ref(stored in PANES ? stored : 'appearance')
const pane = computed(() => PANES[activeTab.value])

// True (safe to leave) unless we're on the Tags tab with unsaved edits, in
// which case confirm with the user before letting anything navigate away,
// registered globally so it also covers leaving the Settings page entirely
// (sidebar nav), not just switching tabs within Settings.
function confirmLeaveTagsIfDirty() {
  if (activeTab.value === 'tags' && tagSettings.isDirty) {
    return window.confirm('You have unsaved tag namespace changes. Leave without saving?')
  }
  return true
}

function selectTab(id) {
  if (id === activeTab.value) return
  if (!confirmLeaveTagsIfDirty()) return
  activeTab.value = id
  try { localStorage.setItem(TAB_KEY, id) } catch { /* storage unavailable */ }
}

onMounted(() => ui.registerNavigationGuard(confirmLeaveTagsIfDirty))
onBeforeUnmount(() => ui.clearNavigationGuard())

const navItem = 'flex items-center gap-2.5 rounded-lg border px-2.5 py-2 text-left font-headline text-[13.5px] font-semibold transition-colors duration-150 [&_.material-symbols-outlined]:text-[19px]'
function itemCls(id) {
  const on = activeTab.value === id
  if (id === 'danger') return on ? 'border-line bg-card text-danger shadow-sm' : 'border-transparent text-danger hover:bg-danger-soft'
  return on
    ? 'border-line bg-card text-fg-strong shadow-sm [&_.material-symbols-outlined]:text-accent'
    : 'border-transparent text-fg-muted hover:bg-chip hover:text-fg'
}
</script>

<template>
  <div class="flex min-w-0 flex-1 flex-col bg-bg">
    <header class="flex h-[60px] shrink-0 items-center gap-4 border-b border-line bg-panel px-6">
      <h1 class="m-0 font-headline text-xl font-bold tracking-[-0.03em] text-fg-strong">Settings</h1>
      <span class="truncate text-[13px] text-fg-faint">Server, sync, and data management</span>
    </header>

    <div class="@container min-h-0 flex-1 overflow-y-auto">
      <div class="flex max-w-[960px] flex-col gap-7 px-8 pb-[60px] pt-6">
        <ServerSection />

        <div class="grid grid-cols-[minmax(0,1fr)] items-start gap-[18px] @min-[720px]:grid-cols-[196px_minmax(0,1fr)] @min-[720px]:gap-8">
          <nav class="flex flex-row flex-wrap gap-1 @min-[720px]:sticky @min-[720px]:top-0 @min-[720px]:flex-col @min-[720px]:gap-[18px]" aria-label="Settings sections">
            <div v-for="g in NAV" :key="g.group" class="flex flex-row gap-0.5 @min-[720px]:flex-col">
              <div class="hidden px-2.5 pb-1.5 font-headline text-[10px] font-semibold uppercase leading-none tracking-[0.18em] text-fg-faint @min-[720px]:block">{{ g.group }}</div>
              <button v-for="it in g.items" :key="it.id" type="button" :aria-current="activeTab === it.id ? 'page' : undefined" :class="[navItem, itemCls(it.id)]" @click="selectTab(it.id)">
                <span class="material-symbols-outlined" aria-hidden="true">{{ it.icon }}</span>{{ it.label }}
              </button>
            </div>
            <div class="flex flex-row @min-[720px]:flex-col @min-[720px]:border-t @min-[720px]:border-line @min-[720px]:pt-4">
              <button type="button" :aria-current="activeTab === 'danger' ? 'page' : undefined" :class="[navItem, itemCls('danger')]" @click="selectTab('danger')">
                <span class="material-symbols-outlined" aria-hidden="true">warning</span>Danger zone
              </button>
            </div>
          </nav>

          <section :key="activeTab" class="flex min-w-0 animate-fade flex-col gap-3.5">
            <div>
              <h2 class="m-0 font-headline text-xl font-bold leading-[1.2] tracking-[-0.03em]" :class="activeTab === 'danger' ? 'text-danger' : 'text-fg-strong'">{{ pane[0] }}</h2>
              <p class="mb-0 mt-1 text-[13px] text-fg-muted">{{ pane[1] }}</p>
            </div>
            <component :is="pane[2]" />
          </section>
        </div>
      </div>
    </div>
  </div>
</template>

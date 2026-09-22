<script setup>
import { ref, onMounted, onBeforeUnmount } from 'vue'
import { useUiStore } from '../stores/ui.js'
import { useTagSettingsStore } from '../stores/tagSettings.js'
import ServerSection from '../components/settings/ServerSection.vue'
import SettingsTabs from '../components/settings/SettingsTabs.vue'
import AppearanceSection from '../components/settings/AppearanceSection.vue'
import SyncSection from '../components/settings/SyncSection.vue'
import HiveSection from '../components/settings/HiveSection.vue'
import TagsSection from '../components/settings/TagsSection.vue'
import LimitsSection from '../components/settings/LimitsSection.vue'
import DataSection from '../components/settings/DataSection.vue'
import DangerSection from '../components/settings/DangerSection.vue'

const ui = useUiStore()
const tagSettings = useTagSettingsStore()

const tabs = [
  { id: 'appearance', label: 'Appearance', component: AppearanceSection },
  { id: 'sync', label: 'Sync', component: SyncSection },
  { id: 'hive', label: 'Hive', component: HiveSection },
  { id: 'tags', label: 'Tags', component: TagsSection },
  { id: 'limits', label: 'Limits', component: LimitsSection },
  { id: 'data', label: 'Data', component: DataSection },
  { id: 'danger', label: 'Danger', component: DangerSection },
]
const activeTab = ref(tabs[0].id)

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
}

onMounted(() => ui.registerNavigationGuard(confirmLeaveTagsIfDirty))
onBeforeUnmount(() => ui.clearNavigationGuard())
</script>

<template>
  <div class="flex-1 overflow-y-auto px-8 py-8">
    <div class="max-w-xl">
      <h2 class="mb-8 font-medium" style="font-size:16px; color:var(--hm-text-primary)">Settings</h2>
      <div class="mb-10">
        <ServerSection />
      </div>
      <SettingsTabs :tabs="tabs" :active="activeTab" @select="selectTab" />
      <component :is="tabs.find(t => t.id === activeTab).component" />
    </div>
  </div>
</template>

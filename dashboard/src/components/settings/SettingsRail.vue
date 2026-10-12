<script setup>
import { computed } from 'vue'
import { useConfigFileStore } from '../../stores/configFile.js'
import ConfigSaveBar from './ConfigSaveBar.vue'

// Right-hand context for the active settings tab: where its values live,
// and for config.toml tabs the lock state and the save controls.
const props = defineProps({ tab: { type: String, required: true }, configTab: Boolean })
const config = useConfigFileStore()

const STORED = {
  appearance: 'Saved in this browser only.',
  hive: 'Hive Mode, paired devices and trusted networks are stored in the Mynd database on this device.',
  tags: 'Stored in the Mynd database and shared with paired Hive devices.',
  limits: 'Stored in the Mynd database.',
  data: 'Exports and imports personal and workspace memories as JSON.',
  danger: 'Actions here cannot be undone.',
}
const showConfig = computed(() => props.configTab && config.loaded)
const card = 'flex flex-col gap-3.5 rounded-xl border border-line bg-card px-[18px] py-4'
const code = 'self-start break-all rounded bg-chip px-1.5 py-0.5 font-[family-name:var(--hm-font-code)] text-xs text-fg-strong'
</script>

<template>
  <aside aria-label="About this section" class="flex flex-col gap-3.5">
    <div v-if="showConfig" :class="card">
      <span class="hm-label mb-0">Saved to</span>
      <span :class="code">{{ config.path }}</span>
      <div class="flex flex-col gap-1">
        <span class="font-headline text-[12.5px] font-semibold text-fg">Editing</span>
        <span v-if="tab === 'agent'" class="text-[12.5px] leading-[1.55] text-fg-muted">The agent runs as a program on this machine, so it can only be edited in config.toml.</span>
        <span v-else-if="config.locked" class="text-[12.5px] leading-[1.55] text-fg-muted">Locked by <span :class="code">allow_config_edit = false</span> under [dashboard].</span>
        <span v-else class="text-[12.5px] leading-[1.55] text-fg-muted">Allowed. Set <span :class="code">allow_config_edit = false</span> under [dashboard] to lock it.</span>
      </div>
      <div class="flex flex-col gap-1">
        <span class="font-headline text-[12.5px] font-semibold text-fg">Applying changes</span>
        <span class="text-[12.5px] leading-[1.55] text-fg-muted">Saved changes take effect after Mynd restarts.</span>
      </div>
      <ConfigSaveBar v-if="tab !== 'agent'" stacked class="border-t border-line pt-3" />
    </div>
    <div v-if="showConfig && tab === 'integrations'" :class="card">
      <span class="hm-label mb-0">Logging in</span>
      <span class="text-[12.5px] leading-[1.55] text-fg-muted">Homeserver, bot user and application are set from the terminal:</span>
      <span :class="code">mynd matrix login</span>
      <span :class="code">mynd discord login</span>
    </div>
    <div v-if="STORED[tab]" :class="card">
      <span class="hm-label mb-0">{{ tab === 'danger' ? 'Careful' : 'Saved to' }}</span>
      <span class="text-[12.5px] leading-[1.55] text-fg-muted">{{ STORED[tab] }}</span>
    </div>
  </aside>
</template>

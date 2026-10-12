<script setup>
import { useConfigFileStore } from '../../stores/configFile.js'
import ConfigSaveBar from './ConfigSaveBar.vue'

// Shell for every config.toml-backed settings pane (SettingsView loads the
// shared draft). The save bar pins under the fields until the settings page
// is wide enough for SettingsRail, which then carries it instead.
const config = useConfigFileStore()
</script>

<template>
  <p v-if="config.loadError === 'forbidden'" class="text-[13px] text-fg-faint">
    config.toml settings are only available from the machine running Mynd.
  </p>
  <p v-else-if="config.loadError" class="text-[13px] text-danger">Could not read config.toml.</p>
  <p v-else-if="!config.loaded" class="text-[13px] text-fg-faint">Loading…</p>
  <template v-else>
    <slot />
    <ConfigSaveBar class="sticky bottom-0 bg-bg py-3 @min-[1180px]:hidden" />
  </template>
</template>

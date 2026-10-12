<script setup>
import { computed } from 'vue'
import { useConfigFileStore } from '../../stores/configFile.js'
import ConfigPane from './ConfigPane.vue'
import ConfigField from './ConfigField.vue'
import SettingRow from './SettingRow.vue'

const config = useConfigFileStore()
const SECTIONS = [
  { id: 'sync', title: 'Cloud sync', env: 'MYND_SYNC_API_KEY' },
  { id: 'org_sync', title: 'Org sync', env: 'MYND_ORG_SYNC_API_KEY' },
]

function keyState(s) {
  const draft = config.apiKeyDrafts[s.id]
  if (draft === null) return 'Will be removed on save'
  if (draft !== undefined) return 'Will be replaced on save'
  return { env: `Set by ${s.env} (overrides the file)`, file: 'Set in config.toml', none: 'Not set' }[config.apiKeys[s.id]]
}
function setKey(id, v) {
  if (v) config.apiKeyDrafts[id] = v
  else delete config.apiKeyDrafts[id]
}
const locked = computed(() => config.locked)
const card = 'divide-y divide-line rounded-xl border border-line bg-card px-[18px] py-1'
</script>

<template>
  <ConfigPane>
    <div v-for="s in SECTIONS" :key="s.id" class="flex flex-col gap-2">
      <span class="hm-label mb-0 px-0.5">{{ s.title }}</span>
      <div :class="card">
        <ConfigField :section="s.id" k="enabled" label="Enabled" type="bool" :placeholder="false" />
        <ConfigField :section="s.id" k="remote_url" label="Remote URL" placeholder="https://your-server.com" />
        <SettingRow label="API key" :hint="keyState(s)">
          <div class="flex items-center gap-2" :title="locked ? 'Locked by [dashboard] allow_config_edit = false in config.toml' : undefined">
            <input type="password" autocomplete="off" :aria-label="s.title + ' API key'" placeholder="Type to replace" :disabled="locked"
              class="hm-input flex-1 font-headline" :value="typeof config.apiKeyDrafts[s.id] === 'string' ? config.apiKeyDrafts[s.id] : ''"
              @change="setKey(s.id, $event.target.value)" />
            <button v-if="config.apiKeys[s.id] === 'file' && config.apiKeyDrafts[s.id] !== null" type="button" class="hm-btn hm-btn-ghost hm-btn-sm" :disabled="locked"
              @click="config.apiKeyDrafts[s.id] = null">Clear</button>
          </div>
        </SettingRow>
        <ConfigField :section="s.id" k="interval_seconds" label="Interval" type="number" placeholder="300" unit="seconds" />
        <ConfigField :section="s.id" k="sync_on_store" label="Sync on store" hint="Push right after a memory is stored." type="bool" :placeholder="true" />
        <ConfigField :section="s.id" k="sync_on_startup" label="Sync on startup" type="bool" :placeholder="true" />
      </div>
    </div>
    <p class="m-0 px-0.5 text-xs text-fg-faint">Cloud sync and Hive Mode are mutually exclusive.</p>
  </ConfigPane>
</template>

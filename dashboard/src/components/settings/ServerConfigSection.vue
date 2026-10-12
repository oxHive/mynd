<script setup>
import ConfigPane from './ConfigPane.vue'
import ConfigField from './ConfigField.vue'

const card = 'divide-y divide-line rounded-xl border border-line bg-card px-[18px] py-1'
</script>

<template>
  <ConfigPane>
    <div class="flex flex-col gap-2">
      <span class="hm-label mb-0 px-0.5">Network</span>
      <div :class="card">
        <ConfigField section="server" k="host" label="API host" hint="0.0.0.0 exposes mynd to your network." placeholder="127.0.0.1" />
        <ConfigField section="server" k="port" label="API port" type="number" placeholder="3456" hint="Hive also binds this +1 and +2." />
        <ConfigField section="dashboard" k="port" label="Dashboard port" type="number" placeholder="3459" />
        <ConfigField section="dashboard" k="api_url" label="API URL" hint="Where the dashboard calls the API." placeholder="http://127.0.0.1:3456" />
        <ConfigField section="dashboard" k="cors_origin" label="CORS origin" hint="Set when opening the dashboard via another hostname." placeholder="http://127.0.0.1:3459" />
      </div>
      <p class="m-0 px-0.5 text-xs text-fg-faint">Changing a host or port moves the dashboard; after restarting, open it at the new address.</p>
    </div>

    <div class="flex flex-col gap-2">
      <span class="hm-label mb-0 px-0.5">Updates</span>
      <div :class="card">
        <ConfigField section="update" k="enabled" label="Check for updates" type="bool" :placeholder="true" />
        <ConfigField section="update" k="check_interval_seconds" label="Check interval" type="number" placeholder="600" unit="seconds" />
        <ConfigField section="update" k="allow_apply_from_api" label="Update from dashboard" hint="Allow the Update button to install and restart." type="bool" :placeholder="true" />
      </div>
    </div>

    <div class="flex flex-col gap-2">
      <span class="hm-label mb-0 px-0.5">Memory</span>
      <div :class="card">
        <ConfigField section="defaults" k="max_inject_tokens" label="Session-start budget" hint="Default for projects that don't set max_tokens." type="number" placeholder="2000" unit="tokens" />
        <ConfigField section="tags" k="guard_predefined_namespaces" label="Lock predefined tags" hint="Protects project, topic, status and the other built-in namespaces." type="bool" :placeholder="true" />
      </div>
    </div>
  </ConfigPane>
</template>

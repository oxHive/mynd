<script setup>
import ConfigPane from './ConfigPane.vue'
import ConfigField from './ConfigField.vue'
import ConfigRows from './ConfigRows.vue'

const GATES = ['manage_guild', 'administrator', 'manage_channels', 'manage_messages', 'kick_members', 'ban_members']
  .map(v => ({ value: v, label: v.replace('_', ' ') }))
const card = 'divide-y divide-line rounded-xl border border-line bg-card px-[18px] py-1'
</script>

<template>
  <ConfigPane>
    <div class="flex flex-col gap-2">
      <span class="hm-label mb-0 px-0.5">Matrix</span>
      <div :class="card">
        <ConfigField section="matrix" k="homeserver_url" label="Homeserver" readonly="Set by mynd matrix login" placeholder="Not logged in" />
        <ConfigField section="matrix" k="user_id" label="Bot user" readonly="Set by mynd matrix login" placeholder="Not logged in" />
        <ConfigField section="matrix" k="allowed_users" label="Allowed users" hint="Comma-separated user IDs that may DM the bot." type="list" placeholder="@you:matrix.org" />
        <ConfigField section="matrix" k="session_ttl_seconds" label="Session TTL" hint="Idle time before a room starts a fresh agent session." type="number" placeholder="120" unit="seconds" />
        <ConfigRows section="matrix" k="rooms" id-key="room_id" id-label="Room" id-placeholder="!abc:matrix.org" />
      </div>
    </div>

    <div class="flex flex-col gap-2">
      <span class="hm-label mb-0 px-0.5">Discord</span>
      <div :class="card">
        <ConfigField section="discord" k="application_id" label="Application" readonly="Set by mynd discord login" placeholder="Not logged in" />
        <ConfigField section="discord" k="allowed_users" label="Allowed users" hint="Comma-separated user IDs that may DM the bot." type="list" placeholder="123456789012345678" />
        <ConfigField section="discord" k="permission_gate" label="/mynd permission" hint="Guild permission required to use /mynd." type="select" placeholder="Everyone" :options="GATES" />
        <ConfigField section="discord" k="session_ttl_seconds" label="Session TTL" type="number" placeholder="120" unit="seconds" />
        <ConfigRows section="discord" k="channels" id-key="channel_id" id-label="Channel" id-placeholder="123456789012345678" />
      </div>
    </div>
  </ConfigPane>
</template>

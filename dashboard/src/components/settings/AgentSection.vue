<script setup>
import ConfigPane from './ConfigPane.vue'
import ConfigField from './ConfigField.vue'

const RO = 'Runs as a program on this machine, so it can only be edited in config.toml'
</script>

<template>
  <ConfigPane>
    <div class="divide-y divide-line rounded-xl border border-line bg-card px-[18px] py-1">
      <ConfigField section="agent" k="command" label="Command" :readonly="RO" placeholder="claude" />
      <ConfigField section="agent" k="args" label="Arguments" type="list" :readonly="RO" placeholder="none" />
      <ConfigField section="agent" k="kind" label="Kind" :readonly="RO" placeholder="detected from command" />
    </div>
  </ConfigPane>
</template>

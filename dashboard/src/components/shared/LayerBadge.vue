<script setup>
import { computed } from 'vue'
const props = defineProps({ layer: String })
const resolved = computed(() => {
  if (props.layer === 'personal') return 'personal'
  if (props.layer === 'org') return 'org'
  return 'workspace'
})
const styleFor = {
  personal: 'background:var(--hm-personal-bg); color:var(--hm-personal)',
  org: 'background:var(--hm-org-bg); color:var(--hm-org)',
  workspace: 'background:var(--hm-workspace-bg); color:var(--hm-workspace)',
}
</script>

<template>
  <span
    class="inline-block rounded-sm px-2 py-0.5 text-[10px] font-mono"
    :style="styleFor[resolved]"
  >{{ resolved }}</span>
</template>

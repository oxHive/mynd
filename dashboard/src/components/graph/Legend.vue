<script setup>
import { PhArrowRight } from '@phosphor-icons/vue'
</script>

<template>
  <div class="flex items-center gap-5 px-4 shrink-0 font-mono"
    style="height:40px; border-top:0.5px solid var(--hm-border-subtle); font-size:10px; color:var(--hm-text-tertiary)">
    <span class="flex items-center gap-1.5">
      <span class="hm-hex" style="width:10px; height:11px; background:var(--hm-personal)"></span>personal
    </span>
    <span class="flex items-center gap-1.5">
      <span class="hm-hex" style="width:10px; height:11px; background:var(--hm-workspace)"></span>workspace
    </span>
    <span class="flex items-center gap-1.5">
      <span class="inline-block w-5 h-0 border-t-2 border-dashed" style="border-color:var(--hm-warning)"></span>pending
    </span>
    <span class="flex items-center gap-1.5">
      <span class="inline-block w-5 h-0 border-t-2" style="border-color:#ff7a45"></span>
      parent<PhArrowRight :size="11" weight="bold" color="#ff7a45" />child
    </span>
    <span class="flex items-center gap-1.5">
      <span class="inline-block w-5 h-0 border-t-2" style="border-color:#c084fc"></span>sibling
    </span>
  </div>
</template>

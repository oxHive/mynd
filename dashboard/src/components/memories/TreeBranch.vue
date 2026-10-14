<script setup>
const props = defineProps({
  node: { type: Object, required: true },
  pointer: { type: String, default: '' },
  children: { type: Map, required: true },
  selectedId: { type: String, default: null },
})
const emit = defineEmits(['open'])
</script>

<template>
  <details v-if="props.children.has(props.node.id)" open>
    <summary class="flex flex-col gap-0.5 cursor-pointer py-1">
      <button class="text-left" :style="props.selectedId === props.node.id ? 'color:var(--hm-accent)' : 'color:var(--hm-text-primary)'"
        @click.prevent="emit('open', props.node.id)">
        {{ props.node.title }}
        <span v-if="props.node.role" class="font-mono" style="font-size:10px; color:var(--hm-text-tertiary)">{{ props.node.role }}</span>
      </button>
      <span v-if="props.pointer" style="font-size:11px; color:var(--hm-text-secondary)">{{ props.pointer }}</span>
    </summary>
    <div class="pl-3" style="border-left:0.5px solid var(--hm-border-subtle)">
      <TreeBranch
        v-for="c in props.children.get(props.node.id)" :key="c.node.id"
        :node="c.node" :pointer="c.pointer" :children="props.children" :selected-id="props.selectedId"
        @open="emit('open', $event)"
      />
    </div>
  </details>
  <div v-else class="flex flex-col gap-0.5 py-1 pl-3">
    <button class="text-left" :style="props.selectedId === props.node.id ? 'color:var(--hm-accent)' : 'color:var(--hm-text-primary)'"
      @click="emit('open', props.node.id)">
      {{ props.node.title }}
    </button>
    <span v-if="props.pointer" style="font-size:11px; color:var(--hm-text-secondary)">{{ props.pointer }}</span>
  </div>
</template>

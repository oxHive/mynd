<script setup>
import { computed } from 'vue'
import { Marked } from 'marked'
import DOMPurify from 'dompurify'
import { useMemoriesStore } from '../../stores/memories.js'

const props = defineProps({ text: { type: String, default: '' } })
const emit = defineEmits(['navigate'])
const memories = useMemoriesStore()

const MEM_LINK_RE = /^(?:(parent|child|sibling):)?(mem_[0-9a-f]{32})$/

const RELATIONSHIP_LINK_COLORS = {
  parent: '#4d9bff',
  child: '#ff7a45',
  sibling: '#c084fc',
}

const md = new Marked({ breaks: true })
md.use({
  renderer: {
    link(token) {
      const match = MEM_LINK_RE.exec(token.href)
      if (match) {
        const kind = match[1] || 'sibling'
        const id = match[2]
        const exists = memories.all.some(m => m.id === id)
        if (exists) {
          const color = RELATIONSHIP_LINK_COLORS[kind]
          return `<a class="hm-mem-link" href="#" data-mem-id="${id}" style="color:${color}">${token.text}</a>`
        }
        return `<span class="hm-mem-link hm-mem-link--dead" title="Memory not found">${token.text}</span>`
      }
      return false
    },
  },
})

const html = computed(() =>
  DOMPurify.sanitize(md.parse(props.text || ''), { ADD_ATTR: ['data-mem-id'] })
)

function onClick(e) {
  const link = e.target.closest('a.hm-mem-link')
  if (link?.dataset.memId) {
    e.preventDefault()
    emit('navigate', link.dataset.memId)
  }
}
</script>

<template>
  <div class="hm-markdown" v-html="html" @click="onClick"></div>
</template>

<style scoped>
.hm-markdown {
  font-size: 12px;
  line-height: 1.6;
  color: var(--fg);
}
.hm-markdown :deep(p) { margin: 0 0 10px; }
.hm-markdown :deep(p:last-child) { margin-bottom: 0; }
.hm-markdown :deep(h1),
.hm-markdown :deep(h2),
.hm-markdown :deep(h3) {
  font-weight: 600;
  margin: 14px 0 6px;
  color: var(--fg-strong);
}
.hm-markdown :deep(h1:first-child),
.hm-markdown :deep(h2:first-child),
.hm-markdown :deep(h3:first-child) { margin-top: 0; }
.hm-markdown :deep(ul) { list-style: disc; margin: 0 0 10px; padding-left: 20px; }
.hm-markdown :deep(ol) { list-style: decimal; margin: 0 0 10px; padding-left: 20px; }
.hm-markdown :deep(li) { display: list-item; margin: 2px 0; }
.hm-markdown :deep(li)::marker { color: var(--hm-text-tertiary); }
.hm-markdown :deep(li:has(input[type="checkbox"])) {
  list-style: none;
  margin-left: -20px;
}
.hm-markdown :deep(input[type="checkbox"]) {
  margin-right: 6px;
  accent-color: var(--hm-accent);
}
.hm-markdown :deep(code) {
  font-family: var(--hm-font-code);
  font-size: 11px;
  background: var(--hm-mono-bg);
  border: 0.5px solid var(--hm-mono-border);
  border-radius: 3px;
  padding: 1px 4px;
}
.hm-markdown :deep(pre) {
  font-family: var(--hm-font-code);
  font-size: 11px;
  background: var(--hm-mono-bg);
  border: 0.5px solid var(--hm-mono-border);
  border-radius: 6px;
  padding: 10px 12px;
  overflow-x: auto;
  margin: 0 0 10px;
}
.hm-markdown :deep(pre code) {
  background: none;
  border: none;
  padding: 0;
}
.hm-markdown :deep(a) {
  color: var(--hm-accent);
}
.hm-markdown :deep(a.hm-mem-link) {
  text-decoration: none;
  border-bottom: 1px dotted currentColor;
  cursor: pointer;
}
.hm-markdown :deep(.hm-mem-link--dead) {
  color: var(--hm-text-tertiary);
  border-bottom: 1px dotted var(--hm-text-tertiary);
  cursor: default;
}
.hm-markdown :deep(blockquote) {
  margin: 0 0 10px;
  padding-left: 10px;
  border-left: 2px solid var(--hm-border-default);
  color: var(--hm-text-secondary);
}
.hm-markdown :deep(table) {
  border-collapse: collapse;
  margin: 0 0 10px;
  font-size: 11px;
}
.hm-markdown :deep(th),
.hm-markdown :deep(td) {
  border: 0.5px solid var(--hm-border-default);
  padding: 4px 8px;
  text-align: left;
}
.hm-markdown :deep(th) {
  background: var(--hm-bg-elevated);
  color: var(--hm-text-secondary);
  font-weight: 600;
}
</style>

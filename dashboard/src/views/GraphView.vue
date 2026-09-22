<script setup>
import { ref, computed } from 'vue'
import { useMemoriesStore } from '../stores/memories.js'
import { useSuggestStore } from '../stores/suggest.js'
import { useGraphStore } from '../stores/graph.js'
import GraphCanvas from '../components/graph/GraphCanvas.vue'
import GraphToolbar from '../components/graph/GraphToolbar.vue'
import PendingBar from '../components/shared/PendingBar.vue'
import DetailPanel from '../components/graph/DetailPanel.vue'
import MiniCard from '../components/graph/MiniCard.vue'
import Legend from '../components/graph/Legend.vue'
import EmptyState from '../components/shared/EmptyState.vue'
import Tooltip from '../components/shared/Tooltip.vue'

const memories = useMemoriesStore()
const suggest = useSuggestStore()
const graph = useGraphStore()

const canvasWrap = ref(null)
const hoveredNode = ref(null)
const hoveredEdge = ref(null)
const mouseX = ref(0)       // viewport-space, for the fixed-position Tooltip
const mouseY = ref(0)
const wrapX = ref(0)        // canvas-wrap-relative, for the absolute-positioned MiniCard
const wrapY = ref(0)
const wrapBounds = ref({ w: 0, h: 0 })

function onCanvasMouseMove(e) {
  mouseX.value = e.clientX
  mouseY.value = e.clientY
  const rect = canvasWrap.value?.getBoundingClientRect()
  if (rect) {
    wrapX.value = e.clientX - rect.left
    wrapY.value = e.clientY - rect.top
    wrapBounds.value = { w: rect.width, h: rect.height }
  }
}

const hoveredMemory = computed(() =>
  hoveredNode.value ? memories.all.find(m => m.id === hoveredNode.value.id) : null
)
const hoveredConnectionCount = computed(() =>
  hoveredNode.value ? graph.edgesFor(hoveredNode.value.id).length : 0
)

const tooltipText = computed(() => {
  if (hoveredEdge.value) return hoveredEdge.value.relationship || 'pending link'
  return ''
})
</script>

<template>
  <div class="flex flex-col flex-1 overflow-hidden">
    <!-- Top bar spans the full graph view, above both the canvas and its detail sidebar -->
    <GraphToolbar />
    <PendingBar />

    <div class="flex flex-1 overflow-hidden">
      <!-- Left: canvas area -->
      <div class="flex flex-col flex-1 overflow-hidden">
        <div ref="canvasWrap" class="flex-1 relative overflow-hidden" @mousemove="onCanvasMouseMove">
          <EmptyState
            v-if="!memories.all.length"
            message="No connections to display."
            hint="Store some memories first; edges appear as they share tags or get linked."
          />
          <GraphCanvas v-else @node-hover="hoveredNode = $event" @edge-hover="hoveredEdge = $event" />

          <MiniCard
            v-if="hoveredNode && hoveredMemory"
            :node="hoveredMemory"
            :x="wrapX"
            :y="wrapY"
            :connection-count="hoveredConnectionCount"
            :bounds="wrapBounds"
          />
        </div>
        <Legend />
      </div>

      <!-- Right: node detail, unless the global suggest panel (App.vue) is open -->
      <DetailPanel v-if="!suggest.panelOpen" />
    </div>

    <Tooltip :visible="!hoveredNode && !!hoveredEdge" :text="tooltipText" :x="mouseX" :y="mouseY" />
  </div>
</template>

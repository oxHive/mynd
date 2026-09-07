<script setup>
import { ref, watch, onMounted, onUnmounted, computed } from 'vue'
import * as d3 from 'd3'
import { useMemoriesStore } from '../../stores/memories.js'
import { useGraphStore } from '../../stores/graph.js'
import { readStored } from '../../lib/localStore.js'

const emit = defineEmits(['node-click', 'node-hover', 'edge-hover'])
const memories = useMemoriesStore()
const graph = useGraphStore()

const canvasEl = ref(null)
const panMode = ref(false)
const hoveredNodeId = ref(null)
let sim = null
let rafId = null
let nodes = []
let links = []
let panning = false
let middlePanning = false
let lastMid = [0, 0]

const CAMERA_KEY = 'mynd.graph.camera'

function loadCamera() {
  try {
    const raw = readStored(CAMERA_KEY)
    if (raw) {
      const p = JSON.parse(raw)
      if (typeof p.x === 'number' && typeof p.y === 'number' && typeof p.k === 'number') return p
    }
  } catch { /* malformed storage, fall through to default */ }
  return { x: 0, y: 0, k: 1 }
}

function saveCamera() {
  localStorage.setItem(CAMERA_KEY, JSON.stringify(transform))
}

const PINNED_KEY = 'mynd.graph.pinned'

function loadPinned() {
  try {
    const raw = readStored(PINNED_KEY)
    return raw ? JSON.parse(raw) : {}
  } catch {
    return {}
  }
}

function savePinnedPosition(id, x, y) {
  const all = loadPinned()
  all[id] = { x, y }
  localStorage.setItem(PINNED_KEY, JSON.stringify(all))
}

let transform = loadCamera()
let zoomBehavior = null

// Single rAF gate for all redraw triggers (sim ticks, zoom, drag, filters).
function scheduleDraw() {
  if (rafId) cancelAnimationFrame(rafId)
  rafId = requestAnimationFrame(draw)
}

// getComputedStyle can force a style recalc; calling it every frame during a
// drag adds measurable jank. Cache the two theme colors and invalidate only
// when the theme actually flips (theme store stamps data-theme on <html>).
let themeColors = null
function getThemeColors() {
  if (!themeColors) {
    const cs = getComputedStyle(document.documentElement)
    themeColors = {
      text: cs.getPropertyValue('--hm-text-primary').trim(),
      draft: cs.getPropertyValue('--hm-warning').trim(),
      halo: cs.getPropertyValue('--hm-bg-base').trim(),
    }
  }
  return themeColors
}

function toWorld(px, py) {
  return [(px - transform.x) / transform.k, (py - transform.y) / transform.k]
}

// clientX/Y → canvas CSS-px coords via the bounding-rect ratio. offsetX/Y is
// wrong here whenever layout scale ≠ visual scale — the app applies CSS zoom
// on <html> (font-scale setting), which shifted every hitbox by the zoom
// factor. The rect ratio is correct under CSS zoom and any devicePixelRatio.
function clientToCanvas(clientX, clientY) {
  const el = canvasEl.value
  const rect = el.getBoundingClientRect()
  return [
    (clientX - rect.left) / rect.width * el.offsetWidth,
    (clientY - rect.top) / rect.height * el.offsetHeight,
  ]
}

function clientToWorld(clientX, clientY) {
  const [px, py] = clientToCanvas(clientX, clientY)
  return toWorld(px, py)
}

function isEditableTarget(el) {
  return !!el && (el.tagName === 'INPUT' || el.tagName === 'TEXTAREA' || el.isContentEditable)
}

function handleKeydown(e) {
  if (isEditableTarget(document.activeElement)) return
  if (e.code === 'Space') {
    e.preventDefault()
    panMode.value = !panMode.value
  } else if (e.key === 'Escape') {
    panMode.value = false
  }
}

const nodeData = computed(() =>
  memories.all.map(m => ({ id: m.id, title: m.title, layer: m.layer, tags: m.tags || [] }))
)

const linkData = computed(() =>
  graph.edges
    .filter(e => e.status === 'active' || e.status === 'pending')
    .map(e => ({ id: e.id, source: e.source_id, target: e.target_id, status: e.status, relationship: e.relationship }))
)

// Camera scale (d3 zoom `transform.k`) below which per-node text is too
// small to read on screen — swap to a single cluster label instead.
const LABEL_MIN_SCALE = 0.55

const COLORS = {
  personal: '#1d9e75',
  workspace: '#7f77dd',
  pending: '#ba7517',
}

// Brighter, higher-contrast than the original muted set — at low alpha and
// 1px width the old colors (#5b8fd9/#c2634a/#9a63d6) were nearly indistinguishable
// from each other and from the background.
// parent and child share a color: the relationship field only records which
// end the edge was stored from ('parent' means target is source's parent,
// 'child' means target is source's child) — the arrowhead (drawn toward the
// child end) is what actually conveys direction, so two colors for the same
// relationship kind was redundant.
const RELATIONSHIP_COLORS = {
  parent: '#ff7a45',
  child: '#ff7a45',
  sibling: '#c084fc',
}
// Pre-existing edges whose relationship predates this taxonomy (shares_tag,
// applies_to, etc.) are left in the DB unmigrated — this is their fallback color.
const DEFAULT_EDGE_COLOR = '#9a9488'

// Degree is cached on each node (set in startSimulation whenever links
// change) instead of scanning `links` on every call — nodeRadius runs once
// per node per animation frame plus once per node per collision-force tick,
// so an O(links) scan there was the main cause of drag-time jank.
function nodeRadius(n) {
  return Math.max(10, 10 + (n.__degree || 0) * 1.5)
}

// Half the on-screen width of a node's label (10px mono ≈ 6px/char, capped
// at the 20-char truncation draw() applies). Collision must reserve this
// space or labels stack on top of each other even when the hexes don't touch.
function labelHalfWidth(n) {
  return Math.min(n.title?.length || 0, 20) * 3
}

function collideRadius(n) {
  return Math.max(nodeRadius(n), labelHalfWidth(n)) + 14
}

// Non-overlapping cluster centers, one circle per project plus one for
// un-projected nodes (keyed null). d3.packSiblings guarantees the circles
// don't intersect, which is what keeps whole clusters from landing on top
// of each other — per-node forces alone can't prevent that.
const CLUSTER_NODE_SPACING = 65
let clusterCenters = new Map()

function computeClusterCenters(nodeList, w, h) {
  const groups = new Map()
  for (const n of nodeList) {
    const key = n.__project ?? null
    if (!groups.has(key)) groups.set(key, 0)
    groups.set(key, groups.get(key) + 1)
  }
  const circles = [...groups.entries()].map(([key, count]) => ({
    key,
    r: CLUSTER_NODE_SPACING * Math.sqrt(count) + 80,
  }))
  d3.packSiblings(circles)
  const centers = new Map()
  for (const c of circles) centers.set(c.key, { x: w / 2 + c.x, y: h / 2 + c.y })
  return centers
}

function centerFor(n) {
  return clusterCenters.get(n.__project ?? null) || { x: 0, y: 0 }
}

function hitTestNode(wx, wy) {
  // Hit padding is specified in screen px and converted to world units via
  // the current zoom — a fixed world-space padding would shrink to a couple
  // screen pixels when zoomed out (world coords get compressed on screen),
  // making the hitbox feel inaccurate at low zoom.
  const pad = 6 / transform.k
  // Reverse order: nodes later in the array draw on top, so when hexes
  // overlap the click should land on the visible (topmost) one.
  for (let i = nodes.length - 1; i >= 0; i--) {
    const node = nodes[i]
    const r = nodeRadius(node) + pad
    const dx = wx - node.x, dy = wy - node.y
    if (dx * dx + dy * dy <= r * r) return node
  }
  return null
}

function projectOf(node) {
  const tag = (node.tags || []).find(t => t.toLowerCase().startsWith('project:'))
  return tag ? tag.slice(tag.indexOf(':') + 1) : null
}

// The project cluster blob + label already identify which project a node
// belongs to — no need to repeat it as a per-node title prefix.
function nodeLabel(node) {
  return { isDraft: memories.isDraft(node.id), text: node.title }
}

// Truncates at the last word boundary at or before `max` instead of
// slicing mid-word — a hard character cut reads as a cut-off word
// ("preferen…"), a word-boundary cut reads as an intentionally shortened
// phrase. Falls back to a hard cut only if there's no space to break on
// (e.g. one long unbroken token).
function truncateLabel(text, max) {
  if (text.length <= max) return text
  const slice = text.slice(0, max)
  const lastSpace = slice.lastIndexOf(' ')
  const cut = lastSpace > max * 0.4 ? slice.slice(0, lastSpace) : slice
  return cut + '…'
}

// Stable pastel color per project name — same string always hashes to the
// same hue so a project's cluster color doesn't shuffle between renders.
const projectColorCache = new Map()
function projectColor(name) {
  if (projectColorCache.has(name)) return projectColorCache.get(name)
  let hash = 0
  for (let i = 0; i < name.length; i++) hash = (hash * 31 + name.charCodeAt(i)) >>> 0
  const hue = hash % 360
  projectColorCache.set(name, hue)
  return hue
}

// Groups nodes by their precomputed __project (set when nodes are built or
// patched) — this runs every frame and every sim tick, so it must not redo
// the per-tag string scanning projectOf does.
function groupByProject(list) {
  const groups = new Map()
  for (const n of list) {
    if (!n.__project) continue
    if (!groups.has(n.__project)) groups.set(n.__project, [])
    groups.get(n.__project).push(n)
  }
  return groups
}

// Draws the cluster as an axis-aligned rounded rect (dashed border) around a
// project's nodes, with the project name + count inside the top-left corner —
// a calculated bounding box instead of a hull-fitted blob, so it reads as a
// deliberate "group boundary" rather than an organic shape.
function drawClusterBox(ctx, name, members, hue, scale) {
  const pad = 30
  let minx = Infinity, miny = Infinity, maxx = -Infinity, maxy = -Infinity
  for (const n of members) {
    const r = nodeRadius(n)
    minx = Math.min(minx, n.x - r)
    miny = Math.min(miny, n.y - r)
    maxx = Math.max(maxx, n.x + r)
    maxy = Math.max(maxy, n.y + r)
  }
  minx -= pad; miny -= pad; maxx += pad; maxy += pad

  const radius = 14
  ctx.beginPath()
  ctx.moveTo(minx + radius, miny)
  ctx.arcTo(maxx, miny, maxx, maxy, radius)
  ctx.arcTo(maxx, maxy, minx, maxy, radius)
  ctx.arcTo(minx, maxy, minx, miny, radius)
  ctx.arcTo(minx, miny, maxx, miny, radius)
  ctx.closePath()

  ctx.fillStyle = `hsla(${hue}, 65%, 55%, 0.12)`
  ctx.fill()
  ctx.strokeStyle = `hsla(${hue}, 65%, 55%, 0.4)`
  ctx.lineWidth = 1.5
  ctx.setLineDash([5, 4])
  ctx.stroke()
  ctx.setLineDash([])

  // Counter-scale the font against the camera zoom so the label stays a
  // roughly constant, readable size on screen as the user scrolls in/out,
  // instead of shrinking or growing along with everything else in world space.
  const screenPx = 12
  const worldPx = Math.min(32, screenPx / Math.max(scale, 0.05))
  ctx.font = `bold ${worldPx}px "IBM Plex Mono", monospace`
  ctx.textAlign = 'left'
  ctx.textBaseline = 'alphabetic'
  ctx.fillStyle = `hsla(${hue}, 70%, 40%, 0.92)`
  ctx.fillText(`${name.toUpperCase()} · ${members.length}`, minx + 10, miny + worldPx + 6)
}

// Draws a filled triangle at `tip`, oriented along the from->tip direction —
// used on parent/child edges to show which end is the child (arrow points
// away from the parent, toward the child), since edge color alone doesn't
// convey direction.
function drawArrowhead(ctx, from, tip, color) {
  const angle = Math.atan2(tip.y - from.y, tip.x - from.x)
  const size = 8
  ctx.beginPath()
  ctx.moveTo(tip.x, tip.y)
  ctx.lineTo(tip.x - size * Math.cos(angle - Math.PI / 6), tip.y - size * Math.sin(angle - Math.PI / 6))
  ctx.lineTo(tip.x - size * Math.cos(angle + Math.PI / 6), tip.y - size * Math.sin(angle + Math.PI / 6))
  ctx.closePath()
  ctx.fillStyle = color
  ctx.fill()
}

// Memories render as hexagonal cells — the honeycomb is the graph.
function traceHex(ctx, x, y, r) {
  ctx.beginPath()
  for (let i = 0; i < 6; i++) {
    const a = (Math.PI / 180) * (60 * i - 90)
    const px = x + r * Math.cos(a)
    const py = y + r * Math.sin(a)
    if (i === 0) ctx.moveTo(px, py)
    else ctx.lineTo(px, py)
  }
  ctx.closePath()
}

function draw() {
  const canvas = canvasEl.value
  if (!canvas) return
  const ctx = canvas.getContext('2d')
  // Backing store is devicePixelRatio-scaled for crisp text; camera math
  // stays in CSS px with the dpr factor baked into the base transform.
  const dpr = window.devicePixelRatio || 1
  ctx.setTransform(1, 0, 0, 1, 0, 0)
  ctx.clearRect(0, 0, canvas.width, canvas.height)

  ctx.save()
  ctx.setTransform(dpr, 0, 0, dpr, 0, 0)
  ctx.translate(transform.x, transform.y)
  ctx.scale(transform.k, transform.k)

  // Below this camera scale, a 10px node label renders under ~5 screen
  // pixels — illegible. Swap per-node titles for one label per cluster.
  const showNodeLabels = transform.k >= LABEL_MIN_SCALE

  // Draw project clusters — soft outline behind everything else.
  const projectGroups = groupByProject(nodes)
  for (const [name, members] of projectGroups) {
    drawClusterBox(ctx, name, members, projectColor(name), transform.k)
  }

  // Neighbor highlight: hovering (falling back to the selected node) dims
  // every node/edge outside that node's 1-hop neighborhood, so the
  // relationship it participates in reads clearly against the rest of the graph.
  const activeId = hoveredNodeId.value || graph.selectedNodeId
  let activeNeighbors = null
  if (activeId) {
    activeNeighbors = new Set([activeId])
    for (const link of links) {
      const sId = link.source?.id, tId = link.target?.id
      if (sId === activeId) activeNeighbors.add(tId)
      if (tId === activeId) activeNeighbors.add(sId)
    }
  }

  // Draw edges — anchored to each node's boundary (not center) along the
  // line between the two centers, so they track node radius/position live.
  for (const link of links) {
    let sx = link.source?.x, sy = link.source?.y
    let tx = link.target?.x, ty = link.target?.y
    if (sx == null || tx == null) continue

    const dx = tx - sx, dy = ty - sy
    const dist = Math.hypot(dx, dy)
    if (dist > 0) {
      const ux = dx / dist, uy = dy / dist
      const sr = nodeRadius(link.source)
      const tr = nodeRadius(link.target)
      sx += ux * sr
      sy += uy * sr
      tx -= ux * tr
      ty -= uy * tr
    }

    ctx.beginPath()
    if (link.status === 'pending') {
      ctx.setLineDash([4, 4])
      ctx.strokeStyle = COLORS.pending
      ctx.globalAlpha = 0.75
      ctx.lineWidth = 2
    } else {
      ctx.setLineDash([])
      ctx.strokeStyle = RELATIONSHIP_COLORS[link.relationship] || DEFAULT_EDGE_COLOR
      ctx.globalAlpha = 0.6
      ctx.lineWidth = 2
    }
    if (graph.selectedEdgeId && link.id === graph.selectedEdgeId) {
      ctx.strokeStyle = COLORS.pending
      ctx.globalAlpha = 1
      ctx.lineWidth = 3.5
    }
    if (activeNeighbors && !(activeNeighbors.has(link.source?.id) && activeNeighbors.has(link.target?.id))) {
      ctx.globalAlpha *= 0.12
    }
    ctx.moveTo(sx, sy)
    ctx.lineTo(tx, ty)
    ctx.stroke()

    // relationship describes target relative to source (e.g. 'parent' means
    // target is source's parent) — see MemoryDetail.vue's INVERSE_RELATIONSHIP.
    // Arrow always points at the child end, away from the parent end, so
    // direction reads correctly regardless of which side is source/target.
    if (link.status !== 'pending' && (link.relationship === 'parent' || link.relationship === 'child')) {
      const parentIsSource = link.relationship === 'child'
      const from = parentIsSource ? { x: sx, y: sy } : { x: tx, y: ty }
      const tip = parentIsSource ? { x: tx, y: ty } : { x: sx, y: sy }
      drawArrowhead(ctx, from, tip, ctx.strokeStyle)
    }

    ctx.globalAlpha = 1
    ctx.setLineDash([])
  }

  // Draw nodes
  const query = graph.searchQuery.trim().toLowerCase()
  const { text: textColor, draft: draftColor, halo: haloColor } = getThemeColors()
  for (const node of nodes) {
    const r = nodeRadius(node)
    const isSelected = graph.selectedNodeId === node.id
    const matchesSearch = !query || node.title.toLowerCase().includes(query)
    const matchesLayer = graph.layerFilter === 'all' || node.layer === graph.layerFilter
    const isMatch = matchesSearch && matchesLayer && graph.matchesTagFilter(node.tags)
    const color = COLORS[node.layer] || COLORS.personal

    // Ring for selected
    if (isSelected) {
      traceHex(ctx, node.x, node.y, r + 3.5)
      ctx.strokeStyle = color
      ctx.globalAlpha = 0.4
      ctx.lineWidth = 1.5
      ctx.stroke()
      ctx.globalAlpha = 1
    }

    const isNeighbor = !activeNeighbors || activeNeighbors.has(node.id)

    traceHex(ctx, node.x, node.y, r)
    ctx.fillStyle = color
    ctx.globalAlpha = !isMatch ? 0.15 : !isNeighbor ? 0.12 : isSelected ? 1 : 0.72
    ctx.fill()
    ctx.globalAlpha = 1

    // Label at zoom >= 2, always for the selected node — suppressed once
    // the camera is zoomed out far enough that text would be illegible.
    if (showNodeLabels && ((graph.zoom >= 2 && isMatch && isNeighbor) || isSelected || node.id === activeId)) {
      const label = nodeLabel(node)
      const text = truncateLabel(label.text, 20)
      const draftPrefix = label.isDraft ? '[DRAFT] ' : ''
      ctx.font = '10px "IBM Plex Mono", monospace'
      ctx.textAlign = 'left'
      const prefixWidth = draftPrefix ? ctx.measureText(draftPrefix).width : 0
      const textWidth = ctx.measureText(text).width
      const startX = node.x - (prefixWidth + textWidth) / 2
      const y = node.y + r + 13
      // Halo keeps labels legible where they cross edges or cluster fills.
      ctx.strokeStyle = haloColor
      ctx.lineWidth = 3
      ctx.lineJoin = 'round'
      if (draftPrefix) ctx.strokeText(draftPrefix, startX, y)
      ctx.strokeText(text, startX + prefixWidth, y)
      if (draftPrefix) {
        ctx.fillStyle = draftColor
        ctx.fillText(draftPrefix, startX, y)
      }
      ctx.fillStyle = textColor
      ctx.fillText(text, startX + prefixWidth, y)
    }
  }

  ctx.restore()
}

let lastTopologyKey = null

// A signature of node/link ids — cheap to compare so we can tell a real
// topology change (a memory or edge added/removed) apart from the SSE
// stream just handing us a same-content-but-new-reference array on every
// backend event. Recreating the simulation on every SSE tick reset alpha to
// 1 each time, which kept re-perturbing every unpinned node forever — most
// visible on edgeless nodes since they had only weak forces holding them,
// while linked nodes got pulled back into place by the link force.
function topologyKey() {
  const nodeIds = nodeData.value.map(n => n.id).sort().join(',')
  const linkIds = linkData.value.map(l => l.id).sort().join(',')
  return `${nodeIds}|${linkIds}`
}

// When node/link ids are unchanged, the sim keeps running on its existing
// arrays — but the store data may still have changed in place: a pending
// edge got accepted (same id, new status), or a memory's title/tags/layer
// got edited. Patch those fields onto the live sim objects so the render
// tracks the store without a full restart.
function syncData() {
  const nodeById = new Map(nodes.map(n => [n.id, n]))
  for (const nd of nodeData.value) {
    const n = nodeById.get(nd.id)
    if (!n) continue
    n.title = nd.title
    n.layer = nd.layer
    n.tags = nd.tags
    n.__project = projectOf(nd)
  }
  const linkById = new Map(links.map(l => [l.id, l]))
  for (const ld of linkData.value) {
    const l = linkById.get(ld.id)
    if (!l) continue
    l.status = ld.status
    l.relationship = ld.relationship
  }
  scheduleDraw()
}

// Once the layout settles every node gets pinned in place. From then on the
// simulation only ever repositions *new* nodes (everything pre-existing is
// fixed), so a drag or an SSE refresh can never make the rest of the graph
// drift — the main complaint with a permanently-live simulation.
function freezeLayout() {
  for (const n of nodes) {
    if (n.fx == null) { n.fx = n.x; n.fy = n.y }
  }
  draw()
}

function startSimulation(force = false) {
  const key = topologyKey()
  if (!force && sim && key === lastTopologyKey) {
    syncData()
    return
  }
  lastTopologyKey = key

  if (sim) sim.stop()
  const canvas = canvasEl.value
  const w = canvas?.offsetWidth || 800
  const h = canvas?.offsetHeight || 600
  const pinned = loadPinned()

  const prevById = new Map(nodes.map(n => [n.id, n]))
  const staged = nodeData.value.map(n => ({ ...n, __project: projectOf(n) }))
  clusterCenters = computeClusterCenters(staged, w, h)

  nodes = staged.map(n => {
    const existing = prevById.get(n.id)
    if (existing) {
      return { ...n, x: existing.x, y: existing.y, vx: existing.vx, vy: existing.vy, fx: existing.fx, fy: existing.fy }
    }
    if (pinned[n.id]) {
      return { ...n, x: pinned[n.id].x, y: pinned[n.id].y, fx: pinned[n.id].x, fy: pinned[n.id].y }
    }
    // New node: seed near its cluster center so the settle pass only has to
    // resolve local overlap instead of hauling it across the canvas.
    const c = centerFor(n)
    return { ...n, x: c.x + (Math.random() - 0.5) * 60, y: c.y + (Math.random() - 0.5) * 60 }
  })

  // Drop links whose endpoints aren't in the node set. The DB cascades edge
  // deletes, but the dashboard refreshes memories and edges independently,
  // so there's a window where an edge references a just-deleted memory —
  // and d3.forceLink throws ("missing: <id>") on any unresolvable link,
  // which would kill the whole simulation.
  const nodeById = new Map(nodes.map(n => [n.id, n]))
  links = linkData.value
    .map(l => ({ ...l, source: nodeById.get(l.source), target: nodeById.get(l.target) }))
    .filter(l => l.source && l.target)

  for (const n of nodes) n.__degree = 0
  for (const l of links) {
    l.source.__degree++
    l.target.__degree++
  }

  sim = d3.forceSimulation(nodes)
    .force('link', d3.forceLink(links).id(d => d.id).distance(130).strength(0.3))
    // distanceMax bounds charge to nearby nodes so repulsion stays local to
    // a cluster instead of pushing whole clusters apart from the packing
    // centers they were assigned.
    .force('charge', d3.forceManyBody().strength(-350).distanceMax(350))
    // Per-node pull toward the node's packed cluster center — this is what
    // holds each project's nodes inside its own non-overlapping circle.
    .force('x', d3.forceX(d => centerFor(d).x).strength(0.08))
    .force('y', d3.forceY(d => centerFor(d).y).strength(0.08))
    // Collision reserves label width, not just the hex — labels were the
    // thing actually overlapping at nodeRadius + 8.
    .force('collision', d3.forceCollide().radius(collideRadius))
    .on('tick', scheduleDraw)
    .on('end', freezeLayout)
}

// Full re-layout on demand: forget pins (user drags included), repack the
// clusters, and let the simulation settle fresh.
function relayout() {
  localStorage.removeItem(PINNED_KEY)
  for (const n of nodes) { n.fx = null; n.fy = null }
  const canvas = canvasEl.value
  clusterCenters = computeClusterCenters(nodes, canvas?.offsetWidth || 800, canvas?.offsetHeight || 600)
  sim?.alpha(1).restart()
}

function handleClick(e) {
  if (panMode.value) return
  const [mx, my] = clientToWorld(e.clientX, e.clientY)
  const node = hitTestNode(mx, my)
  if (node) {
    graph.selectedNodeId = node.id
    emit('node-click', node.id)
    return
  }
  graph.selectedNodeId = null
  graph.selectedEdgeId = null
}

function ptSegDist(px, py, ax, ay, bx, by) {
  const dx = bx - ax, dy = by - ay
  if (dx === 0 && dy === 0) return Math.hypot(px - ax, py - ay)
  const t = Math.max(0, Math.min(1, ((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)))
  return Math.hypot(px - ax - t * dx, py - ay - t * dy)
}

function handleMouseLeave() {
  if (hoveredNodeId.value !== null) {
    hoveredNodeId.value = null
    scheduleDraw()
  }
  emit('node-hover', null)
  emit('edge-hover', null)
}

function handleMouseMove(e) {
  if (panMode.value) {
    canvasEl.value.style.cursor = panning ? 'grabbing' : 'grab'
    if (hoveredNodeId.value !== null) {
      hoveredNodeId.value = null
      scheduleDraw()
    }
    emit('node-hover', null)
    emit('edge-hover', null)
    return
  }
  const [mx, my] = clientToWorld(e.clientX, e.clientY)
  const foundNode = hitTestNode(mx, my)
  if (hoveredNodeId.value !== (foundNode?.id ?? null)) {
    hoveredNodeId.value = foundNode?.id ?? null
    scheduleDraw()
  }
  emit('node-hover', foundNode)

  let foundEdge = null
  if (!foundNode) {
    for (const link of links) {
      if (link.status !== 'pending') continue
      const sx = link.source?.x, sy = link.source?.y
      const tx = link.target?.x, ty = link.target?.y
      if (sx == null || tx == null) continue
      if (ptSegDist(mx, my, sx, sy, tx, ty) <= 8) { foundEdge = link; break }
    }
  }
  emit('edge-hover', foundEdge)

  canvasEl.value.style.cursor = foundNode ? 'pointer' : 'default'
}

// Two-finger trackpad scroll fires as a plain (non-ctrl) wheel event —
// d3.zoom would normally treat that as zoom, same as mouse-wheel. In pan
// mode we want it to pan instead, so it's intercepted here and blocked from
// reaching d3.zoom's own wheel handler via the filter below. A trackpad
// pinch gesture still carries ctrlKey, so that's left alone to zoom.
function handleWheel(e) {
  if (!panMode.value || e.ctrlKey) return
  e.preventDefault()
  const newX = transform.x - e.deltaX
  const newY = transform.y - e.deltaY
  d3.select(canvasEl.value).call(zoomBehavior.transform, d3.zoomIdentity.translate(newX, newY).scale(transform.k))
}

// Middle-mouse-button drag pans regardless of pan-mode state — matches the
// convention in most canvas/design tools where the scroll-wheel click is a
// dedicated pan gesture independent of whichever tool is active.
function handleMouseDown(e) {
  if (e.button !== 1) return
  e.preventDefault()
  middlePanning = true
  lastMid = [e.clientX, e.clientY]
  canvasEl.value.style.cursor = 'grabbing'
}

function handleWindowMouseMove(e) {
  if (!middlePanning) return
  const dx = e.clientX - lastMid[0]
  const dy = e.clientY - lastMid[1]
  lastMid = [e.clientX, e.clientY]
  const newX = transform.x + dx
  const newY = transform.y + dy
  d3.select(canvasEl.value).call(zoomBehavior.transform, d3.zoomIdentity.translate(newX, newY).scale(transform.k))
}

function handleWindowMouseUp(e) {
  if (e.button !== 1 || !middlePanning) return
  middlePanning = false
  saveCamera()
  canvasEl.value.style.cursor = panMode.value ? 'grab' : 'default'
}

let ro = null
let themeObserver = null
onMounted(() => {
  ro = new ResizeObserver(() => {
    const canvas = canvasEl.value
    if (!canvas) return
    const dpr = window.devicePixelRatio || 1
    canvas.width = canvas.offsetWidth * dpr
    canvas.height = canvas.offsetHeight * dpr
    // A resize doesn't change the graph — the settled (frozen) layout stays
    // where it is; just resize the backing store and repaint.
    if (!sim) {
      startSimulation(true)
      return
    }
    scheduleDraw()
  })
  ro.observe(canvasEl.value.parentElement)
  window.addEventListener('keydown', handleKeydown)
  canvasEl.value.addEventListener('wheel', handleWheel, { passive: false })
  canvasEl.value.addEventListener('mousedown', handleMouseDown)
  window.addEventListener('mousemove', handleWindowMouseMove)
  window.addEventListener('mouseup', handleWindowMouseUp)

  // Theme flips restyle CSS variables — drop the cached canvas colors and
  // repaint (the canvas doesn't react to CSS changes on its own).
  themeObserver = new MutationObserver(() => { themeColors = null; scheduleDraw() })
  themeObserver.observe(document.documentElement, { attributes: true, attributeFilter: ['data-theme'] })

  zoomBehavior = d3.zoom()
    .scaleExtent([0.2, 5])
    // In pan mode, a plain wheel event (trackpad two-finger scroll) is
    // handled by handleWheel instead — only a ctrlKey wheel (pinch-zoom)
    // is left for d3.zoom to treat as zoom.
    .filter(event => {
      if (event.type === 'wheel') return !panMode.value || event.ctrlKey
      return panMode.value
    })
    .on('start', event => {
      if (panMode.value && event.sourceEvent?.type !== 'wheel') {
        panning = true
        canvasEl.value.style.cursor = 'grabbing'
      }
    })
    .on('zoom', event => {
      transform.x = event.transform.x
      transform.y = event.transform.y
      transform.k = event.transform.k
      scheduleDraw()
    })
    .on('end', event => {
      saveCamera()
      if (event.sourceEvent?.type !== 'wheel') {
        panning = false
        if (panMode.value) canvasEl.value.style.cursor = 'grab'
      }
    })
  const sel = d3.select(canvasEl.value)
  sel.call(zoomBehavior)
  sel.call(zoomBehavior.transform, d3.zoomIdentity.translate(transform.x, transform.y).scale(transform.k))

  // Dragging never restarts the simulation: after freezeLayout every other
  // node is fixed, so moving one node just moves that node — its edges follow
  // in draw(), and nothing else can drift.
  const dragBehavior = d3.drag()
    .filter(() => !panMode.value)
    .subject(event => {
      const [mx, my] = clientToWorld(event.sourceEvent.clientX, event.sourceEvent.clientY)
      return hitTestNode(mx, my)
    })
    .on('start', event => {
      if (!event.subject) return
      event.subject.fx = event.subject.x
      event.subject.fy = event.subject.y
      event.subject.__dragStartScreen = [event.x, event.y]
    })
    .on('drag', event => {
      if (!event.subject) return
      const [wx, wy] = clientToWorld(event.sourceEvent.clientX, event.sourceEvent.clientY)
      event.subject.fx = wx
      event.subject.fy = wy
      event.subject.x = wx
      event.subject.y = wy
      scheduleDraw()
    })
    .on('end', event => {
      if (!event.subject) return
      const [sx, sy] = event.subject.__dragStartScreen || [event.x, event.y]
      const moved = Math.hypot(event.x - sx, event.y - sy) > 3
      delete event.subject.__dragStartScreen
      if (moved) savePinnedPosition(event.subject.id, event.subject.fx, event.subject.fy)
    })
  sel.call(dragBehavior)
})

onUnmounted(() => {
  ro?.disconnect()
  themeObserver?.disconnect()
  sim?.stop()
  if (rafId) cancelAnimationFrame(rafId)
  window.removeEventListener('keydown', handleKeydown)
  canvasEl.value?.removeEventListener('wheel', handleWheel)
  canvasEl.value?.removeEventListener('mousedown', handleMouseDown)
  window.removeEventListener('mousemove', handleWindowMouseMove)
  window.removeEventListener('mouseup', handleWindowMouseUp)
})

watch([nodeData, linkData], () => startSimulation())
watch(() => graph.relayoutSeq, relayout)
watch([() => graph.zoom, () => graph.searchQuery, () => graph.layerFilter, () => graph.tagFilter, () => graph.selectedNodeId, () => graph.selectedEdgeId], scheduleDraw)
</script>

<template>
  <canvas ref="canvasEl" class="w-full h-full block"
    style="background:var(--hm-bg-base)"
    @click="handleClick"
    @mousemove="handleMouseMove"
    @mouseleave="handleMouseLeave"
  ></canvas>
</template>

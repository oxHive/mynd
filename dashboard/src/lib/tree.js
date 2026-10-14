// Turns a memory_tree response into an outline. Nodes arrive in breadth-first
// order from the root, so the first node pointing at a memory is the one that
// reached it: the memory is placed there, once. That keeps a memory with
// several homes (or two indexes pointing at each other) from repeating or
// recursing forever. Unreached and multi-home memories are also returned as
// `unhomed`, for the audit.
export function buildOutline(tree) {
  const nodes = tree.nodes || []
  const byId = new Map(nodes.map(n => [n.id, n]))
  const root = tree.root_id ? byId.get(tree.root_id) ?? null : null
  const children = new Map()
  const placed = new Set(root ? [root.id] : [])
  for (const n of nodes) {
    if (!n.reachable) continue
    for (const p of n.pointers || []) {
      const target = byId.get(p.target_id)
      if (!target || !target.reachable || placed.has(target.id)) continue
      placed.add(target.id)
      if (!children.has(n.id)) children.set(n.id, [])
      children.get(n.id).push({ node: target, pointer: p.text })
    }
  }
  const unhomed = nodes.filter(
    n => n.id !== root?.id && (!n.reachable || (n.homes || []).length > 1),
  )
  return { root, children, unhomed }
}

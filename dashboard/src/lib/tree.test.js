import { describe, it, expect } from 'vitest'
import { buildOutline } from './tree.js'

const node = (id, pointers = [], extra = {}) => ({
  id, title: id, role: null, tokens: 1, homes: [], updated_at: 0, reachable: true,
  pointers: pointers.map(([target_id, text]) => ({ target_id, text })),
  ...extra,
})

describe('buildOutline', () => {
  it('places each memory once under the first index that points to it, even with a cycle', () => {
    const { root, children, unhomed } = buildOutline({
      root_id: 'r',
      nodes: [
        node('r', [['a', 'to a'], ['b', 'to b']], { homes: ['a'] }),
        node('a', [['c', 'a to c'], ['r', 'back to root']], { homes: ['r'] }),
        node('b', [['c', 'b to c']], { homes: ['r'] }),
        node('c', [], { homes: ['a', 'b'] }),
      ],
    })
    expect(root.id).toBe('r')
    expect(children.get('r').map(c => c.node.id)).toEqual(['a', 'b'])
    expect(children.get('a').map(c => [c.node.id, c.pointer])).toEqual([['c', 'a to c']])
    expect(children.has('b')).toBe(false)
    expect(unhomed.map(n => n.id)).toEqual(['c'])
  })

  it('lists unreached memories as unhomed and copes without a root', () => {
    const { root, children, unhomed } = buildOutline({
      root_id: null,
      nodes: [node('x', [], { reachable: false })],
    })
    expect(root).toBeNull()
    expect(children.size).toBe(0)
    expect(unhomed.map(n => n.id)).toEqual(['x'])
  })
})

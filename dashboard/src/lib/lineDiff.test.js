import { describe, it, expect } from 'vitest'
import { lineDiff } from './lineDiff.js'

describe('lineDiff', () => {
  it('marks new lines and lists removed ones', () => {
    const d = lineDiff('# Sync\n- [old](child:a)\n- [keep](child:b)', '# Sync\n- [keep](child:b)\n- [new](child:c)')
    expect(d.lines).toEqual([
      { text: '# Sync', added: false },
      { text: '- [keep](child:b)', added: false },
      { text: '- [new](child:c)', added: true },
    ])
    expect(d.removed).toEqual(['- [old](child:a)'])
  })

  it('treats a missing before as all new', () => {
    expect(lineDiff(undefined, 'x').lines).toEqual([{ text: 'x', added: true }])
  })
})

// Marks each line of `after` that is not in `before`, and lists the lines of
// `before` that are gone. Set-based rather than a full LCS diff: audit fixes
// add, drop or reword a few pointer lines, so position-exact output is not
// needed to review them.
export function lineDiff(before, after) {
  const b = (before ?? '').split('\n')
  const a = (after ?? '').split('\n')
  const bSet = new Set(before === undefined ? [] : b)
  const aSet = new Set(a)
  return {
    lines: a.map(text => ({ text, added: !bSet.has(text) })),
    removed: before === undefined ? [] : b.filter(text => !aSet.has(text)),
  }
}

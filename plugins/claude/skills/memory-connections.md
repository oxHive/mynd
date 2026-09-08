---
name: memory-connections
description: Show a memory's connections to other memories, grouped by relationship (parent, child, sibling). Pass the memory's ID or title in the argument.
---

Show how a memory connects to the rest of what's stored in Mynd.

Input from $ARGUMENTS: a memory ID or title to look up.

## Process

### 1. Resolve the memory

- If $ARGUMENTS looks like a memory ID (starts with "mem_"), call `memory_recall` with `id`.
- Otherwise call `memory_recall` with `title`.
- If not found, say so and suggest using `/memory-search` to find the right ID.

### 2. Get its connections

Call `memory_get_edges` with the resolved memory's ID.

### 3. Present grouped

Show three sections, always all three even when empty:
- **Parents** (broader context this falls under) — title + id, and the link phrase if present.
- **Children** (specific instances of this) — same format.
- **Siblings** (peers, no hierarchy) — same format.

If a section has no entries, say "(none)" rather than omitting the heading.

A memory's children may be **fragments of one logical document** rather than independent
topics — this is the index/fragment chunking convention (see `/memory-store`'s chunking
step). Don't guess this from phrasing or tag similarity: check the `part` tag directly.
`part:index` on the resolved memory means its children (or at least the ones also tagged
`part:fragment`) are pieces of one document, not separate topics. When that's the case,
say so when presenting the children — e.g. "Children (fragments of this document):" — so
the user understands why several "children" are pieces of one thing, not several unrelated
topics. If the resolved memory has no `part` tag, its children are independent topics as
normal — don't apply the fragment framing speculatively.

## Rules

- Never fabricate connections. Only show what `memory_get_edges` returned.
- If the user wants to explore a connected memory further, offer to `memory_recall` it or run `/memory-connections` on it next.
- A memory's content may contain inline mention links like `[label](mem_xxx)` pointing at
  a parent/child/sibling. Treat these as lazy pointers, not required reading: follow one
  only when the current task actually needs that related memory's content. Don't
  proactively `memory_recall` every mentioned id just because it's linked. This applies
  doubly to `part:fragment` children — recalling every fragment of a document just to
  answer a question about one section defeats the purpose of chunking in the first place.

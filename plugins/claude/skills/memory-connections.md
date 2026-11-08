---
name: memory-connections
description: Show a memory's connections to other memories, grouped by relationship (parent, child, sibling). Pass the memory's ID or title in the argument.
---

Show how a memory connects to the rest of what's stored in Mynd.

Input from $ARGUMENTS: a memory ID or title.

## Process

### 1. Resolve the memory

- $ARGUMENTS starts with `mem_`: `memory_recall` with `id`. Otherwise `memory_recall` with `title`.
- Not found: say so and suggest `/memory-search` to find the right ID.

### 2. Get its connections

Call `memory_get_edges` with the resolved memory's ID.

### 3. Present

Show these four sections, always all four, writing "(none)" under an empty one:

- **Home:** the `parents` entries that have a `link_text`. Each is an index whose content points here with `[link_text](child:...)`. Show title, id and the pointer text.
- **Other parents:** the `parents` entries without a `link_text` (connections stored only as edges). Title and id.
- **Children:** title, id, and link text if any. When the memory is tagged `role:root` or `role:index`, head this section "Pointers (memories filed under this index)".
- **Cross-references:** the `siblings`. Title, id, and link text if any.

Then, for a memory tagged `project:*` and not `role:root`, check the Home count:

- 0: "This memory has no home in the index tree. `/memory-audit` can place it."
- 2 or more: "This memory is filed under N indexes. `/memory-audit` can keep one home and turn the rest into cross-references."

### 4. Offer next steps

Offer to `memory_recall` one connected memory the user picks, or to run `/memory-connections` on it.

## Rules

- Show only what `memory_get_edges` returned.
- Open a connected memory only when the user picks it.

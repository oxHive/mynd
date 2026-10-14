---
name: memory-edit
description: Fetch a memory by ID or title and edit its content or tags. Pass the ID or title in the argument.
---

Edit an existing memory in Mynd and keep its place in the index tree accurate.

Input from $ARGUMENTS: a memory ID or title.

## Process

### 1. Fetch the memory and its home

- $ARGUMENTS starts with `mem_`: `memory_recall` with `id`. Otherwise `memory_recall` with `title`.
- Not found: say so and suggest `/memory-search` to find the right ID.
- Call `memory_get_edges` with its id. The home is the `parents` entry that has a `link_text`; that text is the current pointer. A parent that comes from a `[text](parent:mem_x)` link in this memory's own content (deprecated form) is not a home. A memory with a `project:*` tag and no such parent has no home: mention that `/memory-audit` can place it. The `siblings` are cross-references: `[text](mem_x)` links in either memory's content. They are never a home.

### 2. Show the current state

Show the title, content, tags, and the home's title with the current pointer.

### 3. Ask what to change

Content, title, tags, or a move to another index. Wait for the answer. For a tag change, call `tag_namespaces_list` first and reuse existing namespaces and values.

### 4. Plan the writes

Work out every write before making any:

- **Content or title change:** compare the new content with the home pointer. The pointer no longer matches (a fact in it changed, or the memory now covers more or less): draft a new pointer, one line of roughly 10 to 30 words with no `]`, stating what the memory covers, its key facts, and when to open it. Plan a replacement of that one line in the home.
- **Any new home** (a move, or the new place for a child that loses its line): it must be in the same store as the memory it will hold. Compare the `layer` that `memory_recall` shows for both: `org` lives in the org store, `workspace` and `personal` share the other one. An index in the other store cannot be its home: say so and ask for another.
- **Move to index Y:** plan two writes in this order: first append `- [pointer](child:<this id>)` to Y, then remove this memory's line from the old home. Adding first means a failure in between leaves two homes, never none.
- **The change removes a `child:` pointer line from this memory, whatever its tags** (turning it into a cross-reference counts as removing it): name the memory that would lose its home and ask where it goes: another index, the root, or deletion of that memory too. Plan the line removal only together with that memory's new home, which is written first.
- **The new content adds a `child:` pointer line:** the target becomes this memory's child. A target that already has a home is a move: plan it as **Move to index Y** with this memory as Y. This memory has no `role:index` or `role:root`: add `role:index` in the same update.
- **The new content adds or rewrites a cross-reference `[text](mem_x)`:** the id is a real one returned by a tool, never a title. The text names what the target covers, so a reader can decide from the text alone whether to open it; never "see here", "this", "link" or a bare id. Any memory may hold cross-references, in either store, with no `role` tag. Removing one needs no other write.
- **A tag change removes `role:index` while the content still has `child:` pointers:** keep `role:index` and explain why.
- **The new content is over the size limit** (`memory_update` rejects it): offer `/memory-audit` to split this memory into an index and children.

### 5. Confirm, then apply

Show every planned write: the memory's changed fields, and for each other memory touched, the exact line added, removed or replaced. Wait for confirmation, then apply in this order, so a failure part way never leaves a memory without a home:

1. Every write that adds a pointer line to a new home.
2. `memory_update` on the memory itself (skip when only its home changes). `tags` replaces the whole list: pass every tag you keep plus the changes.
3. Every other write: a pointer line removed or replaced.

For each write to another memory, `memory_recall` it first for its current full content, then `memory_update` it with that content changed only at the planned line.

### 6. Report

"Updated: [title]", the changed fields, and each pointer line added, removed or replaced elsewhere.

## Rules

- Show the current content before changing anything.
- Change only what the user asked for, plus the pointer lines that keep the tree accurate.
- An edit to another memory touches exactly one pointer line; the rest of its content stays byte for byte.

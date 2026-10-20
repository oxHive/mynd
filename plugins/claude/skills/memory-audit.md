---
name: memory-audit
description: Check a project's memory index tree and fix what is wrong with it, with your approval. Pass a project name (default: this project); add "full" to re-check every pointer.
---

Audit one project's memory index tree in Mynd and fix it with the user's approval.

Input from $ARGUMENTS: an optional project name (default: `[project] name` from `.mynd.toml`) and an optional `full`. The user asking about the org layer means `layer: "org"`.

## Process

### 1. Load the tree

Call `memory_tree` with the project (and `layer`). It carries no content: titles, roles, tags, token counts, homes, pointers, cross-references, broken pointers and cross-references, and legacy `parent:` links. Read content with `memory_recall` only where a check below says so.

### 2. Run the checks

<!-- audit-checks:start -->
## Checks

Run every check against the `memory_tree` result. The **pointer rule**: one line, roughly 10 to 30 words, no `]`, stating what the memory covers, its key facts or decisions, and when to open it; never only the title, never "part 2". A **pointer line** is `- [pointer text](child:<id>)`; a **cross-reference** is `[text](<id>)`, inline or on a line of its own (a **cross-reference line**, `- [text](<id>)`). Any memory may hold cross-references, a leaf as well as an index; they never make a home and need no `role` tag. The **cross-reference rule**: the text names what the target covers, so a reader can decide from it alone whether to open it; never "see here", "this", "link" or a bare id; a cross-reference line follows the pointer rule.

1. **No root.** `root_id` is null and the project has memories. Recall every node's content and group the memories by subject. Propose: a root titled `<project> root`, tagged `role:root` and `project:<project>`, with content `# <project>`, one line on what the project is, then the pointer lines for the memories that go directly under it; and one index per group of 3 or more memories, tagged `role:index` plus the group's shared tags, filed under the root with its own pointer, with content `# <group>` plus pointer lines for its members. Groups smaller than 3 go directly under the root. Existing memories are not edited; only the new root and indexes hold the pointer lines.
2. **Orphan.** A node with `reachable: false`. Recall it, pick the index whose pointers best match its subject (or the root), and propose a pointer line for it there.
3. **Several homes.** A node with 2 or more `homes`. Keep the home whose subject fits best; in every other home, propose replacing its pointer line for that node with a cross-reference line with the same text.
4. **Broken pointer or cross-reference.** An id in a node's `broken_pointers`: recall that id; found (it lives in the other store), propose turning the line into a cross-reference line; not found anywhere, propose removing the line. An id in a node's `broken_cross_refs`: recall that id; found (it lives in the other store), the link is valid and stays; not found anywhere, recall the node and propose replacing that `[text](<id>)` link with its plain text.
5. **Stale or vague pointer.** Check each pointer whose target's `updated_at` is newer than its home's `updated_at`; with `full`, check every pointer. Recall the target and compare. Propose a rewrite, following the pointer rule, when the text is only the title, says "part N", or states something the content no longer says. Also check the text of every entry in each node's `cross_refs` against the cross-reference rule; when it fails, recall the target and propose a rewrite of that link's text only.
6. **Oversized index.** An index (`role:index` or `role:root`) over about 400 tokens or with more than about 20 pointers. Propose grouping its pointers into new `role:index` sub-indexes under it; each moved pointer line is added to its sub-index before it is removed here.
7. **Bloated memory.** A memory without a `role` over about 1000 tokens, or whose pointer text names several unrelated subjects. Recall it and propose converting it in place: it keeps its id and gains `role:index`; each subject becomes a new memory carrying that part of the content and the original's tags; the original's content becomes a one-line intro plus a pointer line per new memory. The new memories are stored first, without a parent, and the original is rewritten last, so no content is lost if a write fails.
8. **Tags and legacy structure.** A node with `pointers` and no `role` (a leaf that holds pointer lines) gets `role:index`; its other tags stay. A `part:index` tag becomes `role:index`; a `part:fragment` tag is removed. Each id in `parent_links` gets a pointer line for this memory, added before the `[text](parent:<id>)` link is removed from this memory's content. When the same memory is also an orphan (check 2), this pointer line is its one new home; do not add a second.
9. **Missing cross-references.** From titles and pointer text, find memories in different subtrees that clearly concern the same subject but have no link between them: no pointer and no `cross_refs` entry in either direction. Propose 3 to 7 cross-references, each following the cross-reference rule, with a one-sentence reason. Add none inside a memory that check 10 reports as a duplicate.
10. **Duplicates.** Two memories whose pointer text or content say the same thing. Report the pair and propose nothing for it.
<!-- audit-checks:end -->

### 3. Present fixes in batches

Order: checks 1 to 4 (structure), then 7 and 6 (size), then 8 and 5 (tags, legacy and wording), then 9, then the duplicate report. Show at most 10 fixes per batch. For each fix show what is wrong and every write it makes: new memories (title, tags, parent, pointer, content), and for existing memories the exact lines added, removed or replaced and any tag change. Then wait: the user approves all, picks some, or skips the batch.

### 4. Apply the approved fixes

- A new memory under a parent: `memory_store` with `parent_id` and `pointer`. A new memory whose content already lists its children's pointer lines (a new root or index), or a split-out piece of a bloated memory: `memory_store` as the check describes.
- An existing memory: `memory_recall` it for its current full content, then `memory_update` with the content changed only at the planned lines. `tags` replaces the whole list: pass every tag kept plus the changes.
- Inside one fix, write new memories and new lines before removing old lines or old content.
- After each batch, call `memory_tree` again before the next one; ids and homes have changed.

### 5. Report

Per check: how many fixed, skipped, or reported only. Suggest running `/memory-audit` once more to confirm 0 orphans and 0 memories with several homes.

## Rules

- Never delete a memory; duplicates are reported for the user to handle.
- Every write is shown and approved before it happens.
- Converting a memory into an index keeps its id, so every pointer to it still works.

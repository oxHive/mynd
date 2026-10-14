---
name: memory-search
description: Search Mynd memories by keyword. Returns matching memories with snippets. Use when you need to find something stored previously.
---

Search Mynd for memories matching the query in $ARGUMENTS.

## Process

### 1. Search

Call `memory_search` with the query from $ARGUMENTS. When $ARGUMENTS names a category rather than keywords (for example "sync stuff" or "ideas about billing"), search by tag instead: `memory_search`'s `tags` param takes `namespace:value` entries and ANDs them. Call `tag_namespaces_list` first if you need to confirm the right namespace or value.

### 2. Present results

For each result show the title and ID, a short content snippet, and the tags. Then, by the result's tags:

- `role:root` or `role:index` (legacy: `part:index`): the memory is a map of other memories. Say so and list its pointer lines, `[pointer text](child:mem_x)`, as "pointer text (id)". The snippet cuts the pointers off: `memory_recall` that one index to list them.
- `part:fragment` (legacy): say it is one piece of a larger document filed under an index.
- Anything else: show it as it is.

A result's cross-references, `[text](mem_x)` links in any memory, leaf or index, are listed as "text (id)" when the snippet shows them. The memories behind pointers and cross-references are opened in step 3, on request.

No results: say so and suggest broadening the term.

### 3. Offer next steps

- "Recall the full content of [title]?": `memory_recall` with the ID.
- "Edit [title]?": `/memory-edit`.
- For an index result: "Open [pointer]?", naming the one pointer that best matches what the user is after.

## Rules

- Show at most 10 results; if more exist, note the count and suggest a narrower query.
- Show only what the tools returned.
- Follow a cross-reference only when the current need requires that specific memory; never open every link in a result because it is there.

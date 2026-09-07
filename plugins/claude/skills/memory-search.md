---
name: memory-search
description: Search Mynd memories by keyword. Returns matching memories with snippets. Use when you need to find something stored previously.
---

Search Mynd for memories matching the query in $ARGUMENTS.

## Process

### 1. Search

Call `memory_search` with the query from $ARGUMENTS. If $ARGUMENTS names a category rather than keywords (e.g. "sync stuff", "ideas about billing"), consider searching by tag instead: `memory_search`'s `tags` param takes `namespace:value` entries and ANDs them — call `tag_namespaces_list` first if you need to confirm the right namespace/value.

### 2. Present results

For each result, show:
- Title and ID
- A short content snippet
- Tags

Check each result's tags for `part:index` or `part:fragment` (see `/memory-store`'s
chunking step) rather than guessing from snippet length. `part:index` means this result's
content is short and just links out to fragments via `[phrase](child:mem_xxx)` — the
detail lives in those fragments, not here; say so rather than presenting it as if the
snippet were the whole story. `part:fragment` means this result is itself one piece of a
larger document — worth noting so the user knows more context may exist under its parent.

If no results are found, say so and suggest broadening the search term.

### 3. Offer next steps

After showing results, offer:
- "Recall the full content of [title]?" — call `memory_recall` with the ID
- "Edit [title]?" — use `/memory-edit`

If a result is tagged `part:index`, do not recall or otherwise open its linked fragments
yourself as part of "showing results" — that would load the whole document just to display
a search hit. Only recall a specific fragment if what the user is actually after clearly
points at that one piece (e.g. they ask about the exact section it covers). Never cascade
through every linked fragment "to be thorough."

## Rules

- Show at most 10 results; if more exist, note the count and suggest a narrower query.
- Never fabricate results. Only show what `memory_search` returned.
- Follow mention links (`[phrase](mem_xxx)`) only when the current need actually requires
  that specific related memory — same rule as `/memory-connections`. Don't open every
  mention in a result just because it's there.

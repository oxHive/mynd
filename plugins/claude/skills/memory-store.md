---
name: memory-store
description: Save something to Mynd memory. Pass a title and content, or describe what to remember and Claude will extract and store it.
---

Save a new memory to Mynd.

Input from $ARGUMENTS: a title and content to store, or a free-text description of what to remember. This may be plain text, a URL, or a document — if it's a URL or a document reference, fetch/read its actual content first; size and chunking decisions (step 4) need the real content, not just the reference to it.

## Process

### 1. Parse input

From $ARGUMENTS:
- If the input includes a clear title and body, use them directly.
- Otherwise, extract a short descriptive title (5-10 words) and the full content to store.

Ask the user to confirm the title before storing if it is ambiguous.

### 2. Default the project tag

If `.mynd.toml` exists in the project root, read it and check for a `[project]` table with a `name` key. If present, default the memory's `project:*` tag to `project:<name>` (lowercased). This is a default only — an explicit user instruction about which project to tag (a different name, or no project tag at all) always overrides it. Skip this step if `.mynd.toml` doesn't exist or has no `project.name`.

### 3. Identify tags

Call `tag_namespaces_list` to see the project's registered namespaces (e.g. `project`, `topic`, `status`, `lang`), their allowed values, and their descriptions. Prefer `namespace:value` tags that already fit — reuse an existing value before adding a new one to a namespace, and reuse an existing namespace before reaching for a bare tag. Only use a bare (non-namespaced) tag for something that's genuinely a one-off; if you're about to reuse the same bare tag a second time, that's a sign it belongs in a namespace instead (mention this to the user rather than silently inventing a new namespace).

Check each namespace's `values_mode`: `"fixed"` means `values` is an enforced allow-list — `memory_store` rejects any tag in that namespace using a value not on the list, so pick from the listed values only. `"suggestion"` (or absent) means the list is just a hint — any value is accepted.

Extract 1-3 relevant tags this way from the content, in addition to the project tag from step 2.

### 4. Store — or chunk, if too large

Call `memory_store` with `title`, `content`, and `tags`.

If it's rejected because content exceeds `max_content_tokens` (a dashboard-configurable guardrail, ~1500 tokens by default), the error tells you the actual token count and the limit. Don't just trim the content to fit — split it properly:

1. **Split by structure, not by character count.** Break at existing boundaries — headings, sections, list groups — so each chunk is a coherent unit on its own, not an arbitrary slice. This applies the same way whether the source was plain text, a fetched URL, or a document.
2. **Store each chunk first**, before the index. Each `memory_store` call for a chunk must itself fit under `max_content_tokens` — if a single chunk is still too large, split it further.
   - Tag each chunk with the tag *pattern* from step 3, trimmed to what's actually true of that specific chunk's content — drop tags that don't apply to this chunk, add a chunk-specific one if warranted. Don't blindly copy every index tag onto every chunk.
   - **Exception: `project:*` must be identical across the index and every chunk.** Never vary it per chunk (also enforced by the registry — `project` is a `single_value` namespace).
   - Always add `part:fragment` to every chunk. This is the registered `part` namespace (fixed values `index`/`fragment`, `tag_namespaces_list` describes it) — the explicit, checkable marker that this memory is a piece of something larger. Don't rely on inferring this from phrasing or sparse content; tag it.
   - Each `memory_store` call returns a real `id` (e.g. `mem_a1b2c3...32 hex chars`). Collect these — you'll need them in the next step.
3. **Store the index memory last**, with short content that links to each chunk using the real IDs from step 2:
   ```
   [phrase describing the chunk](child:mem_xxxxxxxxxxxxxxxxxxxxxxxxxxxxxxxx)
   ```
   The index gets the full tag set from step 3 (it's the searchable entry point) plus `part:index` (same `part` namespace as above — `index` and `fragment` are mutually exclusive, a memory is one or the other, never both, never neither once split). One `[phrase](child:mem_xxx)` link per chunk — write a short, specific phrase per link (not a generic "part 2"), since that phrase is what a later reader sees when deciding whether to follow it.

**Critical — do not use `[[title]]`-style wikilinks, ever.** The only syntax that creates a real, working connection is `[phrase](kind:mem_xxx)` with a literal `mem_` ID returned by an actual `memory_store` call. A title, slug, or filename in place of the ID (e.g. `[[pipelines: workflow shared-github-release]]`) parses as plain text — it looks like a link but creates no edge at all, silently. If you don't have the real ID yet (the chunk hasn't been stored), you cannot write the link yet — store order matters (chunks before index) specifically so the ID exists when you need it.

### 5. Confirm

Report back: "Stored: [title] (ID: [id])". If it was chunked, report the index and list each chunk with its title and ID. If the store fails for a reason other than size, report the error.

## Rules

- Never auto-store without showing the user what will be saved.
- Keep titles short and specific enough to be recalled by keyword later.
- The `.mynd.toml`-derived project tag is a default, not a mandate — always defer to what the user explicitly says about tagging.
- When chunking, the goal is that a later reader (agent or human) only loads the specific chunk relevant to what they need — never the whole original document — so keep chunks focused and the index's link phrases specific enough to choose from without opening them.

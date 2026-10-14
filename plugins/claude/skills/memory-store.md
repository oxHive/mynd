---
name: memory-store
description: Save something to Mynd memory. Pass a title and content, or describe what to remember and Claude will extract and store it.
---

Save a new memory to Mynd and file it under the right index in the project's tree.

Input from $ARGUMENTS: a title and content, or a description of what to remember. If it is a URL or a document reference, fetch or read the actual content first; the placement and size decisions below need the real content.

## Process

### 1. Parse input

- A clear title and body are given: use them.
- Otherwise: write a short, specific title (5 to 10 words) and extract the full content.
- The title is ambiguous: ask the user to confirm it.

### 2. Default the project tag

`.mynd.toml` in the project root has `[project] name`: default the tag to `project:<name>`, lowercased. An explicit user instruction about the project tag (another project, or none) wins. No `.mynd.toml` or no name: no default.

### 3. Find the home

The home is the index memory this one is filed under.

1. The memory has no `project:*` tag and is not going to the org layer (for example a personal preference): it needs no home. Skip to step 5 (Pick tags).
2. Find the root. For the workspace layer it is the memory tagged `role:root` and `project:<name>`; for the org layer (`layer: "org"`) it is the org-layer memory tagged `role:root` and `project:<name>`, or, for an org memory with no project tag, the org-layer `role:root` memory with no `project:*` tag.
   - The root is already in context (session start injected it under a `## root: <title> (<id>)` heading or marked it `root: true`, or you recalled it earlier this session): use it as it is.
   - Otherwise: `memory_search` with those tags, then `memory_recall` the match by id.
3. Read the root's pointer lines, `[pointer text](child:mem_x)`. Pick the one whose text covers this content. If that target is an index (`role:index`), `memory_recall` it and repeat this step on its pointers. Recall only the index on your path.
4. Stop at the most specific index that fits. That index is the home. It must be in the same layer's store as the new memory: an org memory gets an org home, a workspace memory a workspace home.
5. No pointer fits at some level: propose two options and let the user pick: the current index as the home, or a new sub-index under it (title, a one-line intro, its own pointer). A new sub-index is stored first, with `role:index`, using step 6 (Write the pointer) and step 7 (Confirm, then store) for its own pointer and parent.
6. No root exists:
   - `memory_search` with `tags: ["project:<name>"]` returns nothing: offer to create the root first, titled `<name> root`, tagged `role:root` and `project:<name>`, with content `# <name>` plus one line on what the project is. It is stored without `parent_id`.
   - The project already has memories: pick tags (step 5), then in one message suggest running `/memory-audit` to build the tree first and show the title, content and tags that would be saved now, without a home. The user answers that they want to store now: that answer is the confirmation for what you showed. Store it without `parent_id` and say the audit will place it.

### 4. Check for a duplicate

Read the pointer lines of the home index from step 3. One already describes this content: tell the user which memory it is and propose `/memory-edit` on it instead. Store a new memory only after the user says it is different.

### 5. Pick tags

Call `tag_namespaces_list`. Reuse existing `namespace:value` tags, and an existing namespace before a new bare tag. A namespace with `values_mode: "fixed"` accepts only its listed values. Add 1 to 3 tags beyond the project tag. A leaf memory gets no `role` tag; `part:*` is deprecated and never used.

### 6. Write the pointer

One line, roughly 10 to 30 words, with no `]`, stating:

- what the memory covers,
- the key facts or decisions inside it,
- when to open it.

Example: `Nightly release flow: tag rules, Homebrew tap bump, Docker digest check; open when changing release.yml`

A later reader decides from this line alone whether to open the memory, so it names the substance, not just the topic.

### 7. Confirm, then store

Show the user the title, content, tags, home path (for example `mynd root › Sync › Hive`) and pointer, then wait for confirmation. Call `memory_store` with `title`, `content`, `tags`, `parent_id` (the home's id) and `pointer`, plus `layer` when it is not the workspace default. The server appends `- [pointer](child:<new id>)` to the home in the same write, so this one call is the whole write.

The response has `parent_oversized: true`: tell the user the home index is now over the size limit and offer `/memory-audit` to split it.

### 8. Content too large

`memory_store` rejects the content for exceeding `max_content_tokens`: split it by structure (headings, sections, list groups), never by character count, and confirm the split with the user once.

1. Store a new index first: a short intro as content, the step 5 tags plus `role:index`, and the `parent_id` and a pointer for the whole document from steps 3 and 6. Keep the returned id.
2. Store each piece with `parent_id` set to that new index's id and its own pointer, specific to that piece. Tag each piece with what is true of it; `project:*` is identical on every piece.
3. A piece is still too large: split it the same way, one level further down.

### 9. Report

"Stored: [title] (ID: [id]) under [home path]". After a split, list the index and each piece with title and id. A store that fails for any reason other than size: report the error as the server gave it.

## Links between memories

- Every link carries a real id returned by a tool: `[text](child:mem_x)` makes a home pointer, `[text](mem_x)` a cross-reference.
- A `[[title]]` wikilink, or a link with a title or slug where the id goes, creates no connection at all.
- Add a cross-reference inline only where the content genuinely refers to another memory.

## Rules

- Show the user what will be saved before every store.
- Keep titles short and specific enough to find by keyword later.
- The `.mynd.toml` project tag is a default; the user's explicit tagging instruction wins.

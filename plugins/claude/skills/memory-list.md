---
name: memory-list
description: List all stored memories with titles, tags, and IDs. Use to browse what Mynd knows before searching or editing.
---

List memories stored in Mynd.

Input from $ARGUMENTS: empty for the flat list, or `tree`, optionally followed by a project name (for example `tree mynd`), for a project's index tree.

## Process

### 1a. Flat list ($ARGUMENTS does not start with `tree`)

Use the MCP prompt `memory-list` if available; otherwise call `memory_search` with a wildcard query.

Group memories by tag when there are more than 10, preferring a registered namespace value (for example all `topic:sync` memories together; call `tag_namespaces_list` if you need the namespace list) over arbitrary bare tags. For each memory show the short ID, the title and the tags.

### 1b. Tree view ($ARGUMENTS starts with `tree`)

1. The project is the name after `tree`, else `[project] name` from `.mynd.toml`.
2. Call `memory_tree` with that `project`.
3. `root_id` is null: say the project has no root index yet, suggest `/memory-audit`, and list the nodes flat.
4. Otherwise render an indented outline starting at the root. Under each node, indent the memories its `pointers` point to, each as `title (short id)` followed by its pointer text. Show each memory once, under the first index that points to it.
5. Then an **Unhomed** section: the nodes with `reachable: false`, plus any node with more than one entry in `homes`, marked "filed under N indexes".

### 2. Offer next steps

- "Search for something specific?": `/memory-search`.
- "Edit a memory?": `/memory-edit [id]`.
- "Delete a memory?": `memory_delete`, after confirmation.
- Unhomed or multi-home memories were listed: "Place them with `/memory-audit`?"

## Rules

- The store is empty: say so and suggest `/memory-store` to add the first entry.
- The tree view never recalls memory content; titles and pointer text are the whole view.

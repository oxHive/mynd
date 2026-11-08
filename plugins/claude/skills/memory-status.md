---
name: memory-status
description: Show Mynd status: total memory count, what will be injected at session start for this project, and token budget usage.
---

Show the current Mynd status for this session and project.

## Process

### 1. Session start preview

Call `mynd_session_start` with the current working directory path. It returns what a fresh session start would inject.

### 2. Index tree

Call `memory_tree` with `project` set to `[project] name` from `.mynd.toml` (skip this step without a `.mynd.toml`).

### 3. Report

- Project name.
- Memories auto-injected at session start, with their token cost; mark the root.
- Token budget used versus total, and whether any entries were skipped.
- Index tree: the root's title and token cost (or "no root index"), the number of memories in the tree, the number of unhomed memories (`reachable: false`), and the number filed under more than one index (`homes` longer than 1).

### 4. Suggest next steps

- Entries were skipped: "Some memories were skipped. Increase max_tokens in .mynd.toml or remove low-value entries from recalls."
- No root index, unhomed memories, or memories with several homes: "Run `/memory-audit` to fix the index tree."
- No recalls configured and a root exists: say nothing about recalls; the root loads on its own.

## Rules

- Report only what the tools returned.

---
name: memory-edit
description: Fetch a memory by ID or title and edit its content or tags. Pass the ID or title in the argument.
---

Edit an existing memory in Mynd.

Input from $ARGUMENTS: a memory ID or title to look up.

## Process

### 1. Fetch the memory

- If $ARGUMENTS looks like a memory ID (starts with "mem_"), call `memory_recall` with `id`.
- Otherwise call `memory_recall` with `title`.
- If not found, say so and suggest using `/memory-search` to find the right ID.

### 2. Show current content

Display the current title, content, and tags clearly so the user can see what exists.

### 3. Ask what to change

Ask the user what they want to update: content, tags, or both. Wait for their response.

If tags are changing, call `tag_namespaces_list` first and steer new tags toward existing namespaces/values (see the `/memory-store` skill's tagging rule) rather than inventing new ones.

### 4. Apply the update

Call `memory_update` with the ID and the new values the user provided. `tags` replaces the full tag list — pass the complete set (existing tags you're keeping plus any changes), not just the diff.

### 5. Confirm

Report: "Updated: [title]". Show the changed fields.

## Rules

- Never overwrite a memory without showing the user the current content first.
- Do not change fields the user did not explicitly mention.

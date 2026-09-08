# Integrating with Mynd

Mynd exposes two integration surfaces: **MCP tools** (for AI agents) and a **REST API** (for scripts, apps, and custom tooling). Both require the server to be running (`mynd up` or the background service).

Base URL: `http://127.0.0.1:3456` (configurable in `~/.config/mynd/config.toml`)

---

## For AI agents (MCP)

Connect your AI client to the MCP endpoint:

```
http://127.0.0.1:3456/mcp
```

Transport: streamable HTTP (MCP 1.x). No authentication required for local connections.

### Tools

#### `mynd_session_start`

Call once at the start of every session. Loads the memories configured in `.mynd.toml` for the current project and returns them as structured JSON within the configured token budget.

**Input:**
```json
{
  "project_path": "/absolute/path/to/project"
}
```

**Output:** JSON with recalled memories, token usage, and skipped entries (if any hit the budget cap).

This is the only tool that reads `.mynd.toml`. All other tools operate on the memory store directly.

---

#### `memory_store`

Store a new memory or overwrite an existing one by ID.

**Input:**
```json
{
  "title": "golang preferences",
  "content": "Always use table-driven tests. Prefer errors.As over type assertions.",
  "tags": ["golang", "testing"],
  "id": "mem_abc123"
}
```

`id` is optional — omit it and Mynd generates one. Tags trigger automatic edge creation between memories that share a tag.

**Output:** `{ "id": "mem_abc123", "stored": true }`

---

#### `memory_recall`

Retrieve a memory by exact title or ID. Falls back to full-text search if the exact match fails.

**Input:**
```json
{
  "query": "golang preferences"
}
```

**Output:** Array of matching memory objects (usually one).

---

#### `memory_search`

Full-text search across all memories using SQLite FTS5.

**Input:**
```json
{
  "query": "postgres indexing",
  "limit": 5
}
```

`limit` defaults to 5, max 20.

**Output:** `{ "count": 2, "results": [...] }`

---

#### `memory_update`

Update the content and/or tags of an existing memory.

**Input:**
```json
{
  "id": "mem_abc123",
  "content": "Updated content goes here.",
  "tags": ["golang", "testing", "updated"]
}
```

**Output:** `{ "updated": true, "id": "mem_abc123" }`

---

#### `memory_delete`

Permanently delete a memory. Cascades to remove its tags and connected edges.

**Input:**
```json
{
  "id": "mem_abc123"
}
```

**Output:** `{ "deleted": true, "id": "mem_abc123" }`

---

#### `memory_store_edge`

Store a confirmed connection between two memories. Use this after you or the user has explicitly decided two memories are related — do not infer edges automatically.

**Input:**
```json
{
  "source_id": "mem_abc123",
  "target_id": "mem_def456",
  "relationship": "applies_to"
}
```

Valid relationships: `shares_tag` | `applies_to` | `pairs_with` | `used_in` | `related_to` | `custom`

Note: edges with `relationship = "shares_tag"` are created automatically by `memory_store` when two memories share a tag. You only need `memory_store_edge` for semantic relationships that go beyond shared tags.

**Output:** `"Edge created: mem_abc123 --[applies_to]--> mem_def456"`

---

### Wiring a new AI client

To give an AI agent access to Mynd:

1. Register the MCP server in the client's config (see `mynd mcp install <client>` or the manual configs in the main README).
2. Add instructions to the agent's system prompt or config file so it knows when to call each tool. A minimal CLAUDE.md block:

```markdown
## Mynd memory

At the start of every session, call `mynd_session_start` with the absolute path of the current project if `.mynd.toml` exists here.

Store memories when the user explicitly asks ("remember this", "store that"). Never auto-store.
Use `memory_recall` or `memory_search` any time the user asks about past context.
```

3. That's it. The agent discovers the available tools from the MCP server at connection time.

---

## REST API

The REST API is available when the server is running (`mynd up`). All endpoints are under `/api/v1/`. All request and response bodies are JSON.

### Status

```http
GET /api/v1/status
```

```json
{
  "version": "0.1.0",
  "memory_count": 42,
  "db_path": "/home/user/.mynd/memories.db",
  "sync": { "enabled": false }
}
```

---

### Memories

#### List memories

```http
GET /api/v1/memories?limit=50&offset=0
```

Returns up to 1000 memories, ordered by `updated_at` descending. Default limit: 200.

```json
{
  "count": 2,
  "memories": [
    {
      "id": "mem_abc123",
      "title": "golang preferences",
      "content": "Always use table-driven tests.",
      "tags": ["golang", "testing"],
      "created_at": 1719187200,
      "updated_at": 1719187200,
      "token_count": 12
    }
  ]
}
```

#### Create a memory

```http
POST /api/v1/memories
Content-Type: application/json

{
  "title": "golang preferences",
  "content": "Always use table-driven tests.",
  "tags": ["golang", "testing"]
}
```

Response `201 Created`:
```json
{ "id": "mem_abc123" }
```

`token_count` is optional — omit it and Mynd will compute it.

#### Get a memory

```http
GET /api/v1/memories/{id}
```

Returns the memory object or `404` if not found.

#### Update a memory

```http
PATCH /api/v1/memories/{id}
Content-Type: application/json

{
  "content": "Updated content.",
  "tags": ["golang"]
}
```

Both fields are optional — omit either to keep the current value.

Response: `{ "updated": true, "id": "mem_abc123" }`

#### Delete a memory

```http
DELETE /api/v1/memories/{id}
```

Response: `{ "deleted": true, "id": "mem_abc123" }`

---

### Search

```http
GET /api/v1/search?q=postgres+indexing&limit=10
```

Full-text search using SQLite FTS5. `limit` defaults to 20, max 50.

```json
{
  "count": 1,
  "results": [{ "id": "mem_xyz", "title": "...", "content": "...", ... }]
}
```

---

### Edges

#### List edges

```http
GET /api/v1/edges
GET /api/v1/edges?memory_id=mem_abc123
```

Pass `memory_id` to filter to edges connected to a specific memory (either direction).

```json
{
  "count": 1,
  "edges": [
    {
      "id": "edge_abc",
      "source_id": "mem_abc123",
      "target_id": "mem_def456",
      "relationship": "applies_to",
      "status": "active",
      "created_at": 1719187200
    }
  ]
}
```

#### Create an edge

```http
POST /api/v1/edges
Content-Type: application/json

{
  "source_id": "mem_abc123",
  "target_id": "mem_def456",
  "relationship": "applies_to"
}
```

Response `201 Created`: `{ "id": "edge_abc" }`

---

### Feedback

Feedback signals let you flag memories for review without modifying them.

#### List feedback

```http
GET /api/v1/feedback
GET /api/v1/feedback?memory_id=mem_abc123
```

#### Submit feedback

```http
POST /api/v1/feedback
Content-Type: application/json

{
  "memory_id": "mem_abc123",
  "signal": "outdated",
  "note": "This no longer applies after the v2 migration."
}
```

`signal` is a free-form string. Suggested values: `outdated`, `wrong`, `redundant`, `needs_detail`.

Response `201 Created`: `{ "id": "fb_xyz" }`

---

### Conflicts

Conflicts are created automatically when sync detects diverging edits to the same memory.

#### List conflicts

```http
GET /api/v1/conflicts
```

#### Resolve a conflict

```http
POST /api/v1/conflicts/{id}/resolve
Content-Type: application/json

{ "resolution": "keep_local" }
```

`resolution` is a free-form string. Suggested values: `keep_local`, `keep_remote`, `merged`.

---

### Sync settings (read-only)

```http
GET /api/v1/settings/sync
```

Returns the current sync settings from `config.toml`. Writing to `POST /api/v1/settings/sync` is a no-op — edit `config.toml` directly and restart.

---

## Memory object shape

All memory objects returned by the API and MCP tools share this shape:

```json
{
  "id": "mem_abc123",
  "title": "string — unique, used for recall by title",
  "content": "string — the full memory text",
  "tags": ["array", "of", "strings"],
  "created_at": 1719187200,
  "updated_at": 1719187200,
  "token_count": 42
}
```

Timestamps are Unix seconds (integer). `token_count` may be `null` if not computed.

---

## Bulk import and migration

There is no batch endpoint. Import memories one at a time using `POST /api/v1/memories` in a loop.

**Title uniqueness:** titles are not enforced unique. Posting the same title twice creates two separate memories with different IDs. If you run a migration script more than once, you will get duplicates. To avoid this, either:

- Run the script once against an empty database, or
- Search by title before inserting and skip if a match exists:

```sh
exists=$(curl -s "http://127.0.0.1:3456/api/v1/search?q=$(python3 -c "import urllib.parse; print(urllib.parse.quote('$TITLE'))")&limit=1" | jq '.count')
if [ "$exists" = "0" ]; then
  curl -s -X POST http://127.0.0.1:3456/api/v1/memories \
    -H "Content-Type: application/json" \
    -d "{\"title\": \"$TITLE\", \"content\": \"$CONTENT\", \"tags\": $TAGS}"
fi
```

**Field mapping:** Mynd memories have three writable fields. Map your existing schema to these:

| Mynd field | Type | Notes |
|----------------|------|-------|
| `title` | string | Unique label used for recall. Use a short, descriptive key. |
| `content` | string | Full memory text. Concatenate multiple fields if needed. |
| `tags` | string array | Use for grouping. Memories that share a tag get edges created automatically. |

**Migration script (Python):**

```python
import sqlite3, requests, json

OLD_DB = "/path/to/your/old.db"
HIVEMIND = "http://127.0.0.1:3456/api/v1"

conn = sqlite3.connect(OLD_DB)
rows = conn.execute("SELECT title, content, tags FROM your_table").fetchall()

skipped, imported = 0, 0
for title, content, tags_raw in rows:
    tags = json.loads(tags_raw) if tags_raw else []

    # Skip if already exists
    existing = requests.get(f"{HIVEMIND}/search", params={"q": title, "limit": 1}).json()
    if existing["count"] > 0:
        skipped += 1
        continue

    requests.post(f"{HIVEMIND}/memories", json={
        "title": title,
        "content": content,
        "tags": tags,
    }).raise_for_status()
    imported += 1

print(f"Done: {imported} imported, {skipped} skipped")
```

Adjust the `SELECT` query and field names to match your schema. The search-before-insert check uses FTS, so it can return false positives on partial matches — add an exact-title check if your titles need strict deduplication.

---

## Example: shell script integration

```sh
#!/bin/sh
# Store a memory from a script
curl -s -X POST http://127.0.0.1:3456/api/v1/memories \
  -H "Content-Type: application/json" \
  -d '{
    "title": "deploy checklist",
    "content": "1. Run migrations. 2. Smoke test /health. 3. Notify #ops.",
    "tags": ["ops", "deploy"]
  }' | jq .

# Search memories
curl -s "http://127.0.0.1:3456/api/v1/search?q=deploy" | jq .results[].title
```

## Example: Python integration

```python
import httplib2, json

BASE = "http://127.0.0.1:3456/api/v1"

def store_memory(title, content, tags=None):
    h = httplib2.Http()
    _, body = h.request(
        f"{BASE}/memories",
        "POST",
        body=json.dumps({"title": title, "content": content, "tags": tags or []}),
        headers={"Content-Type": "application/json"},
    )
    return json.loads(body)

def search(query, limit=5):
    h = httplib2.Http()
    _, body = h.request(f"{BASE}/search?q={query}&limit={limit}")
    return json.loads(body)["results"]
```

Or with `requests`:

```python
import requests

BASE = "http://127.0.0.1:3456/api/v1"

requests.post(f"{BASE}/memories", json={
    "title": "deploy checklist",
    "content": "1. Run migrations. 2. Smoke test. 3. Notify.",
    "tags": ["ops"],
})

results = requests.get(f"{BASE}/search", params={"q": "deploy", "limit": 5}).json()
```

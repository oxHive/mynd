import type { Plugin } from "@opencode-ai/plugin"
import { existsSync, mkdirSync, readdirSync, copyFileSync } from "node:fs"
import { resolve, join } from "node:path"
import { homedir } from "node:os"

const MYND_INSTRUCTIONS = `# Mynd Memory System

You have access to Mynd via MCP tools: memory_store, memory_recall,
memory_search, memory_update, memory_delete, memory_store_edge, mynd_session_start.

At the start of every session, before doing anything else:

1. Check if .mynd.toml exists in the project root.
2. If it exists, call mynd_session_start with the project root path immediately.
3. Incorporate the returned context silently -- do not narrate it.

After calling mynd_session_start:

- If budget.truncated is true, mention once: "Some memory entries were skipped
  due to token budget. Run mynd status to review."
- If any skipped entry has reason not_found, mention once which recalls were not
  found so the user can check their .mynd.toml.
- Then proceed normally.

If .mynd.toml does not exist:

- Do not call mynd_session_start.
- Tools remain available on demand.
- If the user seems to be starting a new project, suggest: "Run mynd init
  to set up memory hooks for this project."

## Suggest storing -- never auto-store

When the user shares something worth persisting (preferences, project context,
design decisions), suggest: "That seems worth remembering -- should I store it?"
Wait for explicit confirmation before calling memory_store.
`

export default (async ({ client, directory, $ }) => {
  const myndBin = await resolveMynd($)
  const installedSkills = installSkills()
  if (installedSkills.length) {
    await client.app.log({
      body: {
        service: "mynd",
        level: "info",
        message: `installed ${installedSkills.length} skill(s) to ${globalSkillsDir()}`,
      },
    })
  }

  if (myndBin) {
    await client.app.log({
      body: {
        service: "mynd",
        level: "info",
        message: `mynd binary found: ${myndBin}`,
      },
    })
  } else {
    await client.app.log({
      body: {
        service: "mynd",
        level: "warn",
        message:
          "mynd binary not found in PATH. MCP server not registered. Install: cargo binstall oxmynd",
      },
    })
  }

  return {
    config: (cfg) => {
      if (!myndBin) return
      if (!cfg.mcp) cfg.mcp = {}
      if (cfg.mcp.mynd) return

      cfg.mcp.mynd = {
        type: "local",
        command: [myndBin],
        enabled: true,
      }
    },

    "experimental.chat.system.transform": async (_input, output) => {
      if (!output.content) output.content = []
      const configPath = resolve(directory, ".mynd.toml")
      if (!existsSync(configPath)) {
        output.content.push(
          "Mynd is available but not initialized for this project. Run: mynd init",
        )
        return
      }

      output.content.push(MYND_INSTRUCTIONS)
    },
  }
}) satisfies Plugin

// OpenCode never scans npm package contents for skills — it only discovers
// them from specific filesystem paths (.opencode/skills, ~/.claude/skills,
// ~/.config/opencode/skills, etc — see https://opencode.ai/docs/skills/).
// So the skills/ bundled in this package are otherwise invisible to
// OpenCode-only users; copy them into its global skills directory on every
// plugin load so they're actually picked up. Overwrites each time (these
// aren't meant to be hand-edited, same as Claude Code plugin skills) so
// updates to this package propagate on next OpenCode start.
function globalSkillsDir(): string {
  const xdg = process.env.XDG_CONFIG_HOME
  const base = xdg && xdg.trim() ? xdg : join(homedir(), ".config")
  return join(base, "opencode", "skills")
}

function installSkills(): string[] {
  try {
    const sourceDir = resolve(import.meta.dir, "skills")
    if (!existsSync(sourceDir)) return []
    const targetRoot = globalSkillsDir()
    const installed: string[] = []
    for (const name of readdirSync(sourceDir)) {
      const sourceSkill = join(sourceDir, name, "SKILL.md")
      if (!existsSync(sourceSkill)) continue
      const targetDir = join(targetRoot, name)
      mkdirSync(targetDir, { recursive: true })
      copyFileSync(sourceSkill, join(targetDir, "SKILL.md"))
      installed.push(name)
    }
    return installed
  } catch {
    return []
  }
}

async function resolveMynd(
  $: (strings: TemplateStringsArray, ...values: unknown[]) => Promise<{ stdout: Uint8Array }>,
): Promise<string | null> {
  try {
    const result = await $`which mynd`
    const path = result.stdout.toString().trim()
    if (path) return path
  } catch {
    // not found
  }
  return null
}

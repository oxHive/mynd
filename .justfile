mod dashboard

import 'recipes/release.just'
import 'recipes/cargo.just'
import 'recipes/keyring.just'
import 'recipes/testenv.just'

_default:
  @just --choose

matrix-login:
  just run "matrix login"

matrix-run:
  just run "matrix run --debug"

matrix-status:
  just run "matrix status"

matrix-test-send:
  cargo run -- matrix send @oxgrad:matrix.org "e2e test from oxley"

up:
  just run up

headless:
  cargo run -- up --headless

status:
  just run status

# Build, install, and register with an AI client for local testing (default: claude)
mcp-install client='claude': install
  mynd mcp install {{client}}

# Force re-register by removing the existing entry first, then reinstalling
mcp-reinstall client='claude': install
  -claude mcp remove {{client}} 2>/dev/null
  mynd mcp install {{client}}

release-major:
  just _release major

release-minor:
  just _release minor

release-patch:
  just _release patch

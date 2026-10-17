---
name: safety-review
description: Adversarial review of a freeMiele change for the three ways it can hurt a user - an appliance acting unasked, a leaked login, a leaked serial number. Use before calling done any change that touches appliance commands (core `put`, `command`, `power`, `brew`, any PUT/POST), the Tauri commands that call them, login, tokens or the Keychain, the event stream, or files in fixtures/. Also use when the user says "safety review".
---

# Safety review

Review a diff as an attacker would, not as its author. Assume it is broken until the code shows
otherwise. Read the code; do not trust the PR text, commit messages or an agent's report.

Get the diff with `git diff main...HEAD` (or the range you were given). Read every changed
function in full, plus every caller of it (`grep -rn <name> crates`).

## The three questions

**1. Can an appliance act without the user asking, right now, for this?**
- Which code paths reach `put` / `command` in `crates/core/src/lib.rs`, or the Tauri commands
  `brew` and `power` in `crates/app/src/main.rs`? Trace each one back to a click or a typed command.
- Retries, reconnects, loops, timers, event handlers, startup and settings changes must never send
  a command. A retry after an error must not repeat a command that may already have worked.
- Does a test, fixture or CI step call the live API or send a command? It must not.
- Is the device ID the one the user chose, never "the first one" by accident?

**2. Can the login leak?**
- Tokens, client ID and secret live only in the Keychain entry `miele-cli`. Look for them in
  logs, error messages, `Debug` output, panics, files, URLs, the web UI, or anything sent to the
  web view.
- Does the change create new tokens where it could reuse the refresh token? Too many tokens can
  get the user's key switched off by Miele.

**3. Can a serial number or private data leak?**
- Real serials (`fabNumber`), `matNumber`, device names and home details must not reach committed
  files, test output, screenshots, `miele dump` output or error text meant for issues.
- New fixtures: only `*.sample.json` is committed, with serials zeroed.
- Run `scripts/checks.sh`; its last step scans for the owner's real serials.

## Report

One line per finding, worst first:

`[STOP|FIX|NOTE] file:line - what goes wrong - the concrete steps that make it happen`

- **STOP**: an appliance can act unasked, or a secret or serial can leak. Blocks the change.
- **FIX**: a real weakness with no direct harm yet (for example, a command path with no test).
- **NOTE**: worth knowing, no action needed.

End with `VERDICT: safe` or `VERDICT: not safe`. If you found nothing, say what you checked.
Do not fix code yourself unless asked; the author fixes.

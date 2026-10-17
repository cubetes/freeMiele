---
name: capture
description: Read-only live capture of what the owner's Miele appliances send through the official API, saved as private fixtures, then compared with what freeMiele parses to list unused fields. Use when the user says "capture", "record what the machine sends", "what data are we missing", or before building a feature on fields we have never seen live.
---

# Capture

Records real API replies so features are built on facts, not guesses. Only reads: it never
starts, stops, powers or changes an appliance. If a question can only be answered by sending a
command, write it down for the owner and stop.

## 1. Snapshot

The owner must be logged in (`cargo run -q -p freemiele-cli -- status` lists the appliances).

```sh
cargo run -q -p freemiele-cli -- dump > fixtures/dump-$(date +%Y%m%d-%H%M).json
```

`dump` reads device, actions, programs, fillingLevels, lastUsedPrograms and failureDetails for
every appliance, and removes serial, index and material numbers and the user-set name. A part
that fails is kept as `{"error": ...}`: that is a finding too (for example 403 MISSING_CONSENT).

The file is git-ignored (`fixtures/*.json`). Never rename it to `*.sample.json`.

Ask the owner what each machine is doing at that moment (off, idle, mid-programme, finished,
fault) and note it: the same field means different things in different states. One snapshot per
interesting state beats many of the same state.

## 2. Live changes (optional)

`cargo run -q -p freemiele-cli -- watch` prints what changes, with the time, while a programme
runs. Run it in the background during a wash or a care programme the owner starts themselves.

## 3. Compare with the code

For each field in the snapshot, check whether `crates/core/src` parses it (`grep -n
'<camelCaseName>\|rename = "<name>"' crates/core/src/*.rs`), and whether the CLI or the app shows
it. Report a table:

| Field | Value seen (state) | Parsed | Shown in CLI | Shown in app | Possible use |

Mark values that never vary across snapshots (always 0, null or -32768): they are not worth
building on. Ignore identifying fields entirely.

## 4. Make a shared sample (only if a test needs one)

Copy only the parts the test needs into `fixtures/<name>.sample.json`, with every serial replaced
by zeros (`000000000000`, or `000000000024` style for a second device). Then run
`scripts/checks.sh`: it fails if a real serial reached a file git would publish.

## Limits

- Raw event-stream lines (`event:` / `data:`) are not captured by any command here; past raw
  captures live in git-ignored `docs/miele-api/captures/`.
- The Data Act API is not covered: freeMiele's access route to it is still unconfirmed.

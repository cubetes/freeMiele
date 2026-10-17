# freeMiele: notes for coding agents

Read [CONTRIBUTING.md](CONTRIBUTING.md) first. Its rules apply to agents too. Design rules are in
[design/README.md](design/README.md).

## Done means checked

- `scripts/checks.sh` is the gate: formatting, tests, clippy and a scan for real serial numbers.
  CI runs the same script. Work is done only when it prints `ALL CHECKS PASSED`.
- Never weaken a check, skip a test or loosen clippy to make it pass.

## Hard stops: ask the owner first, every time

- **Anything that starts, stops, pauses or powers an appliance**, or changes its light, name or
  start time. These are real machines in a real home. Tests and agents never send commands; the
  owner tests them in person.
- **Committing a real serial number, client ID, secret or token.** Real API replies stay in
  git-ignored `fixtures/*.json`; shared samples are `fixtures/*.sample.json` with zeroed serials.
- Pushing, merging, tagging or releasing.

These count as stop points even inside a plan that otherwise runs without pausing.

## Official APIs only in the repo

Public code uses only Miele's official APIs. Any study of Miele's private app or local protocols
is private research: notes go in git-ignored `docs/miele-api/`, never in code, issues or commits.

## Agent workflow

- Superpowers is switched on for this project. Its plans go in `docs/superpowers/` (git-ignored).
- A change that touches appliance commands, login or tokens, or fixtures gets a review with the
  `safety-review` skill before it is called done.
- To see an app state without live data or a login, start the app with a devices file:
  `FREEMIELE_DEMO=fixtures/devices.washer-running.sample.json cargo run -p freemiele-app`.
  Nothing is sent to Miele in demo mode. The `capture` skill records real replies.
- Parallel jobs each own a list of files. Shared files (`crates/core/src/lib.rs`, `Cargo.toml`)
  go to one job that finishes first.

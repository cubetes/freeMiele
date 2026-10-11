# Contributing to freeMiele

Thanks for helping. freeMiele is an unofficial, community project: it is not made or endorsed
by Miele.

## Where the work is

- **Roadmap and what's being worked on:** the [freeMiele project board](https://github.com/users/cubetes/projects/2).
- **Issues:** pick one labelled `good first issue` or `help wanted`, and say in a comment that
  you're taking it.
- **No appliance to test with?** Many issues (docs, design, tests on sample data) don't need one.
- **Have a Miele appliance we don't know yet?** Open an "Appliance support report". It's the most
  useful contribution there is.

## Set up

1. Install Rust (stable).
2. Register your own client ID and secret at <https://developer.miele.com>. Every person needs
   their own; never share them. Self-registered keys are for personal, non-commercial use.
3. `cargo run -- login`, then `cargo run -- status`.

## Rules

- **No secrets or serial numbers in git.** Not in code, tests, fixtures, issues or screenshots.
  Real API replies go in `fixtures/*.json` (git-ignored); shared samples go in
  `fixtures/*.sample.json` with serial numbers replaced by zeros.
- **Tests:** `cargo test` runs against the sample files, never the live API. New logic with a
  branch, a loop or a parser gets a small test.
- **Keep it lean:** the smallest change that fully solves the problem. No new dependency for a
  few lines of code.
- **Design:** use the freeMiele colours and type from the design system. Never use Miele's logo,
  red or fonts.
- **Don't copy Miele's documentation** into the repo. Link to <https://developer.miele.com>.
- **Starting appliances is real.** Never run commands that start, stop or power an appliance in
  tests or CI.

## Licence

freeMiele is AGPL-3.0-or-later. By contributing, you agree your contribution is released under
the same licence.

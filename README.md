<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/banner/freemiele-banner-dark.svg">
  <img src="assets/banner/freemiele-banner.svg" alt="freeMiele: control your Miele appliances from your own computer. Example command: miele brew flat white.">
</picture>

# freeMiele

Control Miele appliances from your Mac's terminal: see their status, turn them on and off,
and brew drinks. Built first for the CM 6160 coffee machine; washer-dryers are next.

Unofficial. Not made or endorsed by Miele. Uses Miele's public cloud API.

See [supported appliances](docs/appliances.md) and the [roadmap](https://github.com/users/cubetes/projects/2).

## Before you start

1. Pair your appliance in the Miele app, so it appears on your Miele account.
2. Register at <https://developer.miele.com> to get your own **client ID** and
   **client secret**. Every user needs their own; never commit or share them.

## Install

Download the file for your computer from the [latest release](https://github.com/cubetes/freeMiele/releases/latest),
unpack it, and put `miele` somewhere on your `PATH`. Or, with [Rust](https://rustup.rs) installed:

```sh
cargo install --git https://github.com/cubetes/freeMiele
```

On a Mac, a downloaded `miele` is blocked the first time because it isn't signed by Apple. Allow it once
with `xattr -d com.apple.quarantine miele`.

Your login is kept in the macOS Keychain, so only the macOS builds can remember it today.

## Use

```sh
miele login            # asks for client ID and secret, then opens the browser
miele status
miele on               # turns on whichever appliance allows it
miele off
miele brew             # lists the drinks your machine offers
miele brew flat white
miele brew --device CM6160 espresso   # pick the appliance by name or model if you have several
miele care             # how often your coffee machine has been cleaned, and the care schedule
miele logout           # ends the login at Miele and deletes it from this computer
```

Put a cup under the spout before `brew`: the machine starts straight away.

Miele only allows a remote drink when **Mobile Start** is on at the machine. It seems to
switch off after each drink, so turn it on again each time. `status` shows
"(mobile start off)" when it is needed.

Your login is stored in the macOS Keychain (entry `miele-cli`), never in a file.

## Contribute

See [CONTRIBUTING.md](CONTRIBUTING.md) and the [project board](https://github.com/users/cubetes/projects/2).

## Help us support more appliances

What an appliance can do over the API differs by model. To add support for yours, save
its replies (after `login`, using the token from Miele's API test page):

- `GET /devices` → `fixtures/devices.json`
- `GET /devices/{id}/actions` → `fixtures/actions.json`
- `GET /devices/{id}/programs` → `fixtures/programs.json`

These real files are git-ignored because they contain serial numbers. To share one,
copy it to `*.sample.json` and replace the serial number with zeros.

## Logo

In `assets/logo/`: `freemiele-mark.svg` (light), `freemiele-mark-dark.svg` (dark),
`freemiele-mark-small.svg` (for 16–32 px, e.g. favicons), `freemiele-icon-512.png`, and the
name lockup `freemiele-logo.svg` / `freemiele-logo-dark.svg` (the name is drawn as shapes, so
no font is needed). Banners for this README and GitHub's social preview are in `assets/banner/`.
Teal and copper only; never Miele's red or logo.

## Develop

```sh
cargo test --workspace
cargo clippy --workspace
```

Tests run against `fixtures/*.sample.json`, never the live API.

## Licence

AGPL-3.0-or-later. See [LICENSE](LICENSE). You may use, change and share this code, but
anything you build on it and share (including as an online service) must also be AGPL.

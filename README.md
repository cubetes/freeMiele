<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo/freemiele-mark-dark.svg">
  <img src="assets/logo/freemiele-mark.svg" alt="freeMiele logo: a teal tile with a copper dial" width="96">
</picture>

# freeMiele

Control Miele appliances from your Mac's terminal: see their status, turn them on and off,
and brew drinks. Built first for the CM 6160 coffee machine; washer-dryers are next.

Unofficial. Not made or endorsed by Miele. Uses Miele's public cloud API.

## Before you start

1. Pair your appliance in the Miele app, so it appears on your Miele account.
2. Register at <https://developer.miele.com> to get your own **client ID** and
   **client secret**. Every user needs their own; never commit or share them.

## Use

```sh
cargo run -- login            # asks for client ID and secret, then opens the browser
cargo run -- login --legacy   # if the login page says your country isn't supported
cargo run -- status
cargo run -- on               # turns on whichever appliance allows it
cargo run -- off
cargo run -- brew             # lists the drinks your machine offers
cargo run -- brew flat white
```

Put a cup under the spout before `brew`: the machine starts straight away.

Miele only allows a remote drink when **Mobile Start** is on at the machine. It seems to
switch off after each drink, so turn it on again each time. `status` shows
"(mobile start off)" when it is needed.

Your login is stored in the macOS Keychain (entry `miele-cli`), never in a file.

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
`freemiele-mark-small.svg` (for 16–32 px, e.g. favicons) and `freemiele-icon-512.png`.
Teal and copper only; never Miele's red or logo.

## Develop

```sh
cargo test
cargo clippy
```

Tests run against `fixtures/*.sample.json`, never the live API.

## Licence

AGPL-3.0-or-later. See [LICENSE](LICENSE). You may use, change and share this code, but
anything you build on it and share (including as an online service) must also be AGPL.

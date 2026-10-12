# Security

freeMiele handles your Miele login (client ID, client secret and access tokens) and can start
real appliances, so security problems matter here.

## Reporting a problem

Please **don't open a public issue**. Report it privately through GitHub:
[Security → Report a vulnerability](https://github.com/cubetes/freeMiele/security/advisories/new).

Say what you found, how to reproduce it, and what could go wrong. Never include your own
client secret, tokens or serial numbers.

You'll get an answer within about a week. Security reports go ahead of all other work.

## Supported versions

Only the [latest release](https://github.com/cubetes/freeMiele/releases/latest) gets fixes.

## What counts

- Anything that could leak a client secret, token or serial number
- Anything that could start, stop or power an appliance without the user asking
- Problems in how the login is stored (macOS Keychain) or sent to Miele

Problems in Miele's own API or app belong with Miele, not here.

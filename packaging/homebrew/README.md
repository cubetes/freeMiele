# Homebrew files

Templates for the tap `cubetes/homebrew-freemiele` (create that repo by hand):
`Formula/miele.rb` (the command) and `Casks/freemiele.rb` (the app). The `@...@` values are
filled with the version and sha256 of each release file.

The release workflow does this on every tag, but only if a `HOMEBREW_TAP_TOKEN` secret exists
(a token that can push to the tap repo). Without it, copy the files by hand and fill the values.

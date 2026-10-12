# Template: the release workflow fills the @...@ values and copies this to the tap.
cask "freemiele" do
  version "@VERSION@"
  sha256 "@SHA_APP@"

  url "https://github.com/cubetes/freeMiele/releases/download/v#{version}/freeMiele-v#{version}-macos.zip"
  name "freeMiele"
  desc "Menu-bar app for Miele appliances"
  homepage "https://github.com/cubetes/freeMiele"

  app "freeMiele.app"

  caveats <<~EOS
    freeMiele is not signed by Apple. The first time, macOS will refuse to open it.
    Allow it once with:
      xattr -d com.apple.quarantine /Applications/freeMiele.app
    Then log in with the `miele` command first (brew install cubetes/freemiele/miele; miele login).
  EOS
end

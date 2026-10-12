# Template: the release workflow fills the @...@ values and copies this to the tap.
class Miele < Formula
  desc "Command line for Miele appliances, using the official Miele API"
  homepage "https://github.com/cubetes/freeMiele"
  version "@VERSION@"
  license "AGPL-3.0-or-later"

  on_macos do
    on_arm do
      url "https://github.com/cubetes/freeMiele/releases/download/v#{version}/miele-v#{version}-aarch64-apple-darwin.tar.gz"
      sha256 "@SHA_MAC_ARM@"
    end
    on_intel do
      url "https://github.com/cubetes/freeMiele/releases/download/v#{version}/miele-v#{version}-x86_64-apple-darwin.tar.gz"
      sha256 "@SHA_MAC_INTEL@"
    end
  end

  on_linux do
    on_intel do
      url "https://github.com/cubetes/freeMiele/releases/download/v#{version}/miele-v#{version}-x86_64-unknown-linux-gnu.tar.gz"
      sha256 "@SHA_LINUX@"
    end
  end

  def install
    bin.install "miele"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/miele --version")
  end
end

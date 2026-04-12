# Template for Homebrew Cask submission.
# TODO: Submit to https://github.com/Homebrew/homebrew-cask when releasing a macOS build.
# Replace all TODO placeholders with real values before submitting.

cask "vortex" do
  version "TODO"  # e.g. "1.0.0"

  on_intel do
    sha256 "TODO"  # sha256 of the x64 .dmg
    url "https://github.com/mpiton/vortex/releases/download/v#{version}/Vortex_#{version}_x64.dmg"
  end

  on_arm do
    sha256 "TODO"  # sha256 of the aarch64 .dmg
    url "https://github.com/mpiton/vortex/releases/download/v#{version}/Vortex_#{version}_aarch64.dmg"
  end

  name "Vortex"
  desc "Open-source download manager"
  homepage "https://github.com/mpiton/vortex"

  livecheck do
    url :url
    strategy :github_latest
  end

  app "Vortex.app"

  zap trash: [
    "~/Library/Application Support/dev.vortex.app",
    "~/.config/vortex",
  ]
end

# TODO: Submit to https://github.com/Homebrew/homebrew-cask when releasing a macOS build.
# Replace all TODO placeholders with real values before submitting.

cask "vortex" do
  version "TODO"  # e.g. "1.0.0"
  sha256 "TODO"   # sha256 of the .dmg file

  url "https://github.com/mpiton/vortex/releases/download/v#{version}/Vortex_#{version}_x64.dmg"
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

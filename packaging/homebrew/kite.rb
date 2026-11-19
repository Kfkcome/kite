# Cask template: scripts/release.sh fills @VERSION@ and @SHA256@ and writes the result to
# target/release/bundle/kite.rb. Copy that into a tap (Kfkcome/homebrew-kite, Casks/kite.rb);
# then `brew install --cask Kfkcome/kite/kite`.
cask "kite" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "https://github.com/Kfkcome/kite/releases/download/v#{version}/kite-#{version}-macos-arm64.zip"
  name "Kite"
  desc "Fast, low-memory SSH terminal"
  homepage "https://github.com/Kfkcome/kite"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: :big_sur

  app "Kite.app"

  zap trash: [
    "~/Library/Application Support/kite",
    "~/Library/Logs/kite",
  ]
end

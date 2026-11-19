# Cask template: scripts/release.sh fills @VERSION@ and @SHA256@ and writes the result to
# target/release/bundle/kite.rb. Copy that into a tap (your-moon/homebrew-tern, Casks/kite.rb);
# then `brew install --cask your-moon/tern/kite`.
cask "kite" do
  version "@VERSION@"
  sha256 "@SHA256@"

  url "https://github.com/your-moon/tern/releases/download/v#{version}/kite-#{version}-macos-arm64.zip"
  name "kite"
  desc "Fast, low-memory SSH terminal"
  homepage "https://github.com/your-moon/tern"

  livecheck do
    url :url
    strategy :github_latest
  end

  depends_on arch: :arm64
  depends_on macos: :big_sur

  app "kite.app"

  zap trash: [
    "~/Library/Application Support/tern",
    "~/Library/Logs/tern",
  ]
end

class Asp < Formula
  desc "Durable, branchable workspaces for AI agents"
  homepage "https://github.com/ArnavBorkar/agentspaces"
  version "0.1.1"
  license any_of: ["MIT", "Apache-2.0"]

  on_macos do
    on_arm do
      url "https://github.com/ArnavBorkar/agentspaces/releases/download/v0.1.1/asp-v0.1.1-aarch64-apple-darwin.tar.gz"
      sha256 "a105a90822024a7383f2991b4dad1be4a89c95fea2336c25dd7051a2dea7e03a"
    end

    on_intel do
      url "https://github.com/ArnavBorkar/agentspaces/releases/download/v0.1.1/asp-v0.1.1-x86_64-apple-darwin.tar.gz"
      sha256 "7d195d178a78b4b67d3f9a50b386c76c5a01703bd208f4f9dfcc9cd687659b14"
    end
  end

  on_linux do
    on_arm do
      url "https://github.com/ArnavBorkar/agentspaces/releases/download/v0.1.1/asp-v0.1.1-aarch64-unknown-linux-musl.tar.gz"
      sha256 "f3076d02108b1abf921b7abd2241c815b58eb1ed20d5ef5b842cda484a0add98"
    end

    on_intel do
      url "https://github.com/ArnavBorkar/agentspaces/releases/download/v0.1.1/asp-v0.1.1-x86_64-unknown-linux-musl.tar.gz"
      sha256 "60b8ec2fe0d93acbb13a86c00a3f4676ba2749559b1a65055d0f7f9e37cc9ad2"
    end
  end

  depends_on "git"

  def install
    bin.install "asp"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/asp --version")
    assert_match "schemaVersion", shell_output("#{bin}/asp --json schema")
  end
end

#!/bin/sh
# Writes the Homebrew formula for a release to stdout.
#
#   packaging/homebrew/formula.sh VERSION DIST_DIR
#
# DIST_DIR holds the release archives and their .sha256 files, as the
# release workflow builds them. The formula installs prebuilt binaries;
# publish it in a tap (a repository named homebrew-<tap>).
set -eu
version=${1:?usage: formula.sh VERSION DIST_DIR}
dist=${2:?usage: formula.sh VERSION DIST_DIR}
repo=${GRAFT_REPO:-404khai/graft}
sha() { cut -d' ' -f1 < "$dist/graft-$version-$1.tar.gz.sha256"; }
url() { echo "https://github.com/$repo/releases/download/v$version/graft-$version-$1.tar.gz"; }
cat <<RUBY
class Graft < Formula
  desc "Copy-on-write workspaces for parallel coding agents"
  homepage "https://github.com/$repo"
  license any_of: ["MIT", "Apache-2.0"]
  version "$version"

  on_macos do
    on_arm do
      url "$(url aarch64-apple-darwin)"
      sha256 "$(sha aarch64-apple-darwin)"
    end
  end

  on_linux do
    on_intel do
      url "$(url x86_64-unknown-linux-gnu)"
      sha256 "$(sha x86_64-unknown-linux-gnu)"
    end
    on_arm do
      url "$(url aarch64-unknown-linux-gnu)"
      sha256 "$(sha aarch64-unknown-linux-gnu)"
    end
  end

  def install
    bin.install "graft", "graftd"
  end

  test do
    assert_match version.to_s, shell_output("#{bin}/graft --version")
  end
end
RUBY

#!/usr/bin/env bash
# Points the Nix packages at a release's binaries and desktop bundles.
# Usage: update-nix-package.sh [--only-newer] [version]  (default: the latest GitHub release)
# With --only-newer it leaves packages of the same or a newer version alone, as the release
# workflow does: an older tag's run finishing last, or run again, mustn't downgrade them.
set -euo pipefail

cd "$(dirname "$0")/.."
package_files=(nix/package.nix nix/desktop-package.nix)
repo_url="https://github.com/buntec/stereo"

only_newer=false
if [[ "${1:-}" == --only-newer ]]; then
  only_newer=true
  shift
fi
version="${1:-}"
version="${version#v}"

if [[ -z "$version" ]]; then
  echo "looking up the latest GitHub release"
  version="$({
    curl --fail --location --silent --show-error --retry 5 --retry-delay 20 \
      --header 'Accept: application/vnd.github+json' \
      https://api.github.com/repos/buntec/stereo/releases/latest
  } | sed -n 's/.*"tag_name": *"v\([^"]*\)".*/\1/p')"
fi

if [[ -z "$version" ]]; then
  echo "error: could not determine the latest GitHub release" >&2
  exit 1
fi

if [[ ! "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "error: not a release version: $version" >&2
  exit 1
fi

if $only_newer; then
  for package_file in "${package_files[@]}"; do
    current="$(sed -n 's/^  version = "\(.*\)";/\1/p' "$package_file")"
    newest="$(printf '%s\n%s\n' "$current" "$version" | sort -t. -k1,1n -k2,2n -k3,3n | tail -1)"
    if [[ "$current" == "$version" || "$newest" != "$version" ]]; then
      echo "$package_file is at $current already, not older than $version: leaving it"
      exit 0
    fi
  done
fi

# In the order of the packages' `platforms`.
systems=(aarch64-darwin x86_64-darwin aarch64-linux x86_64-linux)
platforms=(macos-aarch64 macos-x86_64 linux-aarch64 linux-x86_64)

tmp_dir="$(mktemp -d)"
trap 'rm -rf "$tmp_dir"' EXIT

for package_file in "${package_files[@]}"; do
  for i in "${!systems[@]}"; do
    name="stereo-${platforms[$i]}"
    if [[ "$package_file" == nix/desktop-package.nix ]]; then
      extension=deb
      [[ "${systems[$i]}" != *darwin ]] || extension=dmg
      name="stereo-desktop-${platforms[$i]}.$extension"
    fi
    url="$repo_url/releases/download/v$version/$name"
    echo "fetching $url"
    curl --fail --location --silent --show-error --retry 5 --retry-delay 20 \
      --output "$tmp_dir/$name" "$url"
    # For awk, as e.g. hash_aarch64_darwin.
    var="hash_${systems[$i]//-/_}"
    export "$var=$(nix hash file --type sha256 --sri "$tmp_dir/$name")"
  done

  awk -v version="$version" '
    /^  version = / { print "  version = \"" version "\";"; next }
    /^  hashes = \{/ { in_hashes = 1 }
    in_hashes && match($0, /"[a-z0-9_]+-(darwin|linux)"/) {
      sys = substr($0, RSTART + 1, RLENGTH - 2)
      var = "hash_" sys
      gsub("-", "_", var)
      print "    \"" sys "\" = \"" ENVIRON[var] "\";"
      next
    }
    in_hashes && /^  \};/ { in_hashes = 0 }
    { print }
  ' "$package_file" > "$tmp_dir/$(basename "$package_file")"
done

# Only once every download succeeded.
for package_file in "${package_files[@]}"; do
  mv "$tmp_dir/$(basename "$package_file")" "$package_file"
done

echo "updated the Nix packages to $version"

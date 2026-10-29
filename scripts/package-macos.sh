#!/bin/bash
set -euo pipefail
cd "$(dirname "$0")/.."
if [[ "$(uname -s)" != Darwin || "$(uname -m)" != arm64 ]]; then
    echo 'Packaging requires an Apple Silicon Mac.' >&2
    exit 1
fi
export PATH="$HOME/.cargo/bin:$PATH"
export MACOSX_DEPLOYMENT_TARGET=26.0
if [[ -z "${DEVELOPER_DIR:-}" && -d /Applications/Xcode.app/Contents/Developer ]]; then
    export DEVELOPER_DIR=/Applications/Xcode.app/Contents/Developer
fi
version="$(sed -n 's/^version = "\([^"]*\)"/\1/p' crates/usage-app/Cargo.toml | head -1)"
[[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+(-[a-zA-Z0-9.]+)?$ ]] || { echo 'Invalid package version.' >&2; exit 1; }
cargo build --locked --release -p usage-app -p usage-panel
target_dir="${CARGO_TARGET_DIR:-target}"
stage="$(mktemp -d "${TMPDIR:-/tmp}/light-usage-bar-package.XXXXXX")"
trap 'rm -rf "$stage"' EXIT
app="$stage/Light Usage Bar.app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Helpers" "$app/Contents/Resources" dist
install -m 755 "$target_dir/release/light-usage-bar" "$app/Contents/MacOS/light-usage-bar"
install -m 755 "$target_dir/release/light-usage-panel" "$app/Contents/Helpers/light-usage-panel"
sed "s/@VERSION@/$version/g" packaging/Info.plist > "$app/Contents/Info.plist"
cargo metadata --locked --format-version 1 --filter-platform aarch64-apple-darwin > "$stage/dependencies.json"
cargo run --locked -p notice-collector -- "$stage/dependencies.json" "$app/Contents/Resources/ThirdPartyNotices" packaging/licenses
install -m 644 README.md "$app/Contents/Resources/README.md"
/usr/bin/ditto docs "$app/Contents/Resources/docs"
/usr/bin/plutil -lint "$app/Contents/Info.plist"
identity="${SIGNING_IDENTITY:--}"
if [[ "$identity" == - ]]; then
    /usr/bin/codesign --force --sign - "$app/Contents/Helpers/light-usage-panel"
    /usr/bin/codesign --force --sign - "$app"
else
    /usr/bin/codesign --force --options runtime --timestamp --sign "$identity" "$app/Contents/Helpers/light-usage-panel"
    /usr/bin/codesign --force --options runtime --timestamp --sign "$identity" "$app"
fi
/usr/bin/codesign --verify --deep --strict --verbose=2 "$app"
archive="dist/Light-Usage-Bar-${version}-macOS-arm64.zip"
/usr/bin/ditto -c -k --sequesterRsrc --keepParent "$app" "$archive"
(cd dist && /usr/bin/shasum -a 256 "$(basename "$archive")" > "$(basename "$archive").sha256")
rm -rf -- 'dist/Light Usage Bar.app'
/usr/bin/ditto "$app" 'dist/Light Usage Bar.app'
/usr/bin/codesign --verify --deep --strict --verbose=2 'dist/Light Usage Bar.app'
echo "$archive"

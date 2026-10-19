#!/bin/sh
here="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
sys="$(uname -s)"
case "$sys" in
MINGW* | MSYS* | CYGWIN*) rel="x86_64-pc-windows-msvc/gray-prompt.exe" ;;
Darwin*)
    case "$(uname -m)" in
    arm64) rel="aarch64-apple-darwin/gray-prompt" ;;
    *) rel="x86_64-apple-darwin/gray-prompt" ;;
    esac ;;
*)
    case "$(uname -m)" in
    aarch64 | arm64) rel="aarch64-unknown-linux-musl/gray-prompt" ;;
    *) rel="x86_64-unknown-linux-musl/gray-prompt" ;;
    esac ;;
esac
if [ -x "$here/$rel" ]; then exec "$here/$rel" "$@"; fi
for cand in "$here/target/release/gray-prompt.exe" "$here/target/release/gray-prompt" "$here/target/debug/gray-prompt.exe" "$here/target/debug/gray-prompt"; do
    if [ -x "$cand" ]; then exec "$cand" "$@"; fi
done
echo "gray-prompt binary not found in $here" >&2
exit 1

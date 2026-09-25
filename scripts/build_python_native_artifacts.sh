#!/usr/bin/env sh
# M034 wheel matrix: build abi3 wheels for the locally supported macOS
# targets plus an sdist, then import-smoke each wheel artifact.
set -eu
OUT="${1:-dist wheels}"
mkdir -p "$OUT"
(cd bindings/python-native && python3 -m maturin build --out "$OUT")
# Cross-built Apple wheels are only produced on a Darwin host; on
# Linux/Windows the host wheel above is the runtime-qualified artifact.
if [ "$(uname -s)" = "Darwin" ]; then
  if rustup target list --installed 2>/dev/null | grep -q x86_64-apple-darwin; then
    (cd bindings/python-native && python3 -m maturin build --target x86_64-apple-darwin --out "$OUT")
  fi
  if rustup target list --installed 2>/dev/null | grep -q aarch64-apple-darwin; then
    (cd bindings/python-native && python3 -m maturin build --target aarch64-apple-darwin --out "$OUT")
  fi
fi
(cd bindings/python-native && python3 -m maturin sdist --out "$OUT")
for wheel in "$OUT"/*.whl "$OUT"/*.tar.gz; do
  echo "artifact: $(basename "$wheel")"
done
NATIVE_ARCH="$(python3 -c 'import platform; print(platform.machine())')"
case "$NATIVE_ARCH" in
  x86_64) NATIVE_TAG="x86_64" ;;
  arm64|aarch64) NATIVE_TAG="arm64" ;;
  *) NATIVE_TAG="$NATIVE_ARCH" ;;
esac
NATIVE_WHEEL="$(ls "$OUT"/*.whl | grep -E "$NATIVE_TAG|universal2" | head -1)"
if [ -z "$NATIVE_WHEEL" ]; then
  echo "no wheel matching interpreter arch $NATIVE_ARCH"; ls "$OUT"; exit 1
fi
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM
pip install --quiet --target "$WORK/pylibs" --no-deps "$NATIVE_WHEEL"
PYTHONPATH="$WORK/pylibs" python3 -c "
import eggchaos_native as n
with n.Service(seed=1) as s:
    assert s.health()['running'] is True
    p = s.create_proxy(name='smoke', listen='127.0.0.1:0', upstream='127.0.0.1:9')
    assert p['proxy']['running'] is True
print('import smoke ok on $NATIVE_ARCH')"
echo '{"python_native_artifacts":"pass"}'

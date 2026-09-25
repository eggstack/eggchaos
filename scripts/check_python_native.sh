#!/usr/bin/env sh
# M034 native binding gate: Rust facade tests, unsafe-boundary audit,
# wheel build, and server-independent Python tests.
set -eu
cargo test -p eggchaos-embed --all-features
# The binding crate stands outside the workspace (plain cargo cannot link
# a macOS extension-module cdylib; maturin owns that step).
cargo test --manifest-path bindings/python-native/Cargo.toml --all-features
cargo audit --file bindings/python-native/Cargo.lock --deny warnings
echo "--- unsafe audit (no handwritten unsafe blocks/fns/impls permitted) ---"
if grep -rnE "unsafe[[:space:]]*(\{|\(|fn|impl|trait|extern)" bindings/python-native/src; then
  echo "handwritten unsafe in binding sources"; exit 1;
fi
grep -n "allow(unsafe_code)" bindings/python-native/src/lib.rs
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM
NATIVE_TARGET=""
if [ "$(python3 -c 'import platform; print(platform.machine())')" = "x86_64" ]; then
  NATIVE_TARGET="--target x86_64-apple-darwin"
fi
(cd bindings/python-native && python3 -m maturin build $NATIVE_TARGET --out "$WORK/wheels")
WHEEL="$(ls "$WORK"/wheels/*.whl | head -1)"
python3 -c "import zipfile,sys; names = zipfile.ZipFile('$WHEEL').namelist(); assert any(n.endswith('.abi3.so') for n in names), names; print('abi3 wheel ok')"
pip install --quiet --target "$WORK/pylibs" --no-deps "$WHEEL"
PYTHONPATH="$WORK/pylibs" python3 -m pytest bindings/python-native/tests/test_native.py -q
echo '{"python_native":"pass"}'

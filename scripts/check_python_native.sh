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
# M059: narrowest compiler-enforceable PyO3 boundary. No crate-root
# inner allowance may exist; the macro-facing module allows exactly its
# generated FFI glue while the crate root and every ordinary
# implementation module deny unsafe.
if grep -n '#!\[allow(unsafe_code)\]' bindings/python-native/src/lib.rs; then
  echo "crate-root allow(unsafe_code) forbidden; keep the boundary scoped"; exit 1;
fi
grep -n '#!\[allow(unsafe_code)\]' bindings/python-native/src/bridge.rs \
  || { echo "macro layer must scope its generated-glue allowance"; exit 1; }
grep -n "deny(unsafe_code)" bindings/python-native/src/lib.rs \
  || { echo "crate root must deny unsafe_code"; exit 1; }
grep -n "deny(unsafe_code)" bindings/python-native/src/convert.rs \
  || { echo "safe helper module must deny unsafe_code"; exit 1; }
WORK="$(mktemp -d)"
trap 'rm -rf "$WORK"' EXIT INT TERM
# Host-aware target selection: derive from OS + architecture. Only Darwin
# interpreters may select an Apple target; Linux/Windows always use
# native-host builds. Override with EGGCHAOS_NATIVE_TARGET for intentional
# cross builds (not runtime-qualified without a matching import).
NATIVE_TARGET=""
HOST_OS="$(uname -s)"
HOST_ARCH="$(python3 -c 'import platform; print(platform.machine())')"
case "$HOST_OS" in
  Darwin)
    case "$HOST_ARCH" in
      x86_64) NATIVE_TARGET="--target x86_64-apple-darwin" ;;
    esac
    ;;
esac
if [ -n "${EGGCHAOS_NATIVE_TARGET:-}" ]; then
  NATIVE_TARGET="--target $EGGCHAOS_NATIVE_TARGET"
fi
# M059: --locked honors the committed python-native Cargo.lock
# explicitly: lock drift fails instead of silently re-resolving.
(cd bindings/python-native && python3 -m maturin build --locked $NATIVE_TARGET --out "$WORK/wheels")
WHEEL="$(ls "$WORK"/wheels/*.whl | head -1)"
python3 -c "import zipfile,sys; names = zipfile.ZipFile('$WHEEL').namelist(); assert any(n.endswith('.abi3.so') for n in names), names; print('abi3 wheel ok')"
pip install --quiet --target "$WORK/pylibs" --no-deps "$WHEEL"
PYTHONPATH="$WORK/pylibs" python3 -m pytest bindings/python-native/tests/test_native.py -q
echo '{"python_native":"pass"}'

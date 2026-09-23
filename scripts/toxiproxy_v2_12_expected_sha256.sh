#!/usr/bin/env sh
set -eu

case "$(uname -s):$(uname -m)" in
  Linux:x86_64) printf '%s\n' '556d891134a3c582dc1e1a3f7335fd55142e5965769855a00b944e13e48302fc' ;;
  Linux:aarch64) printf '%s\n' '53e770c1c3035b5a9f1bc629fce537db1f95f62b26f4ebe6e756afd701cf077c' ;;
  Darwin:arm64|Darwin:aarch64) printf '%s\n' 'aa299966b52f16a8594f1cd0d1e9049dc2e8fe2c04a90c19860e2719b2b95d15' ;;
  Darwin:x86_64) printf '%s\n' '9625bba4bd96117eedae49f982aba4c2f462b268dd406c9ff18186f9b1ef8afe' ;;
  *) exit 2 ;;
esac

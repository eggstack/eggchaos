#!/usr/bin/env sh
set -eu
cargo fmt --all -- --check
cargo clippy --workspace --all-targets --all-features -- -D warnings
cargo test --workspace --all-features
cargo doc --workspace --all-features --no-deps
cargo build --workspace --release
cargo audit --deny warnings
cargo deny check advisories licenses bans sources
cargo package -p eggchaos-core --allow-dirty
cargo package -p eggchaos-core --list --allow-dirty
cargo package -p eggchaos-experiment --list --allow-dirty
cargo package -p eggchaos-protocol --list --allow-dirty
cargo package -p eggchaos-server --list --allow-dirty
cargo package -p eggchaos-eggfetch --list --allow-dirty
cargo package -p eggchaos-toxiproxy --list --allow-dirty
cargo package -p eggchaos-embed --list --allow-dirty
cargo package -p eggchaos-cli --list --allow-dirty
cargo build --release --locked --package eggchaos-cli
./scripts/release-artifact-smoke.sh
# Order-publishability proof for dependents. A dependent's full
# `cargo package` resolves its intra-workspace deps from the registry, so
# it can only succeed after its predecessors publish (observed pre-
# publication: "no matching package named eggchaos-core found"). Contents
# are proven above via `cargo package --list` for every crate; here assert
# that every intra-workspace path dependency carries the workspace version
# requirement, so publishing in the documented order
# (core -> experiment/eggfetch -> protocol -> server/toxiproxy/cli -> embed) resolves from the registry.
# After each predecessor publishes, its dependents' full `cargo package`
# succeeds; that post-publication check is an owner release-step action.
python3 - <<'EOF'
import json, subprocess, sys
meta = json.loads(subprocess.run(
    ["cargo", "metadata", "--format-version", "1", "--locked"],
    capture_output=True, text=True, check=True).stdout)
ws_version = next(p["version"] for p in meta["packages"] if p["name"] == "eggchaos-core")
assert ws_version, "workspace version not found"
problems = []
for p in meta["packages"]:
    if not p["name"].startswith("eggchaos-"):
        continue
    for d in p["dependencies"]:
        if d.get("path") and d["name"].startswith("eggchaos-"):
            if d.get("req") != f"^{ws_version}":
                problems.append(f'{p["name"]} -> {d["name"]}: req {d.get("req")!r}')
            if d.get("source") is not None and "crates.io" not in str(d.get("source")):
                problems.append(f'{p["name"]} -> {d["name"]}: non-registry source')
if problems:
    print("ORDER-PROOF FAIL:")
    print("\n".join(problems))
    sys.exit(1)
print(f'{{"order_proof":"pass","workspace_version":"{ws_version}","order":"core->experiment/eggfetch->protocol->server/toxiproxy/cli->embed"}}')
EOF

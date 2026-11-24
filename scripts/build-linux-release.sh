#!/bin/sh
set -eu

# The Service keeps its GNU libc baseline; the Agent is static musl so the
# same probe artifact runs on both GNU/Linux and Alpine without gcompat.
# Bind mounts require a local Linux Docker daemon, not a remote desktop context.
[ "$(uname -s)" = Linux ] || { echo "requires a local Linux Docker runner" >&2; exit 1; }
prepare_only=0
case "${1:-}" in
  "") ;;
  --prepare-builder) prepare_only=1 ;;
  *) echo "usage: $0 [--prepare-builder]" >&2; exit 1 ;;
esac
if [ "$prepare_only" -eq 0 ]; then
  : "${SENLUN_BUILD_GIT_HASH:?set the source revision}"
fi
: "${SENLUN_BUILD_TARGET:?set the native Rust target}"
repository=$(CDPATH='' cd -- "$(dirname -- "$0")/.." && pwd)
builder_base=rust:1.99-bookworm@sha256:e6f2cff35d0f309acf8433b4f05878ed77092602c5065cc81801177da0bb3a9c
case "$SENLUN_BUILD_TARGET" in
 x86_64-unknown-linux-gnu) musl_target=x86_64-unknown-linux-musl ;;
 aarch64-unknown-linux-gnu) musl_target=aarch64-unknown-linux-musl ;;
 *) echo 'unsupported native release target' >&2; exit 1 ;;
esac
builder="senlun-native-builder:$SENLUN_BUILD_TARGET"
docker build --build-arg "RUST_BASE=$builder_base" --build-arg "MUSL_TARGET=$musl_target" \
 --file "$repository/scripts/Dockerfile.native-builder" --tag "$builder" "$repository/scripts"
if [ "$prepare_only" -eq 1 ]; then
  exit 0
fi
runtime=debian:bookworm-slim@sha256:88200866dfff7ea7f5cbcb6ec7c8a701889efe6fe859fe64d6990e4b07ea4171
cargo_home=${CARGO_HOME:-$HOME/.cargo}
mkdir -p "$cargo_home"

# Share the runner's restored registry with Docker and leave cache files readable
# by the runner. Keep absolute paths stable for Cargo dependency fingerprints.
docker run --rm \
  --user "$(id -u):$(id -g)" \
  --env SENLUN_BUILD_GIT_HASH --env SENLUN_BUILD_TARGET --env "SENLUN_MUSL_TARGET=$musl_target" \
  --env "CARGO_HOME=$cargo_home" \
  --env CARGO_INCREMENTAL=0 --env RUSTUP_TOOLCHAIN=1.99.0 \
  --mount "type=bind,src=$cargo_home,dst=$cargo_home" \
  --mount "type=bind,src=$repository,dst=$repository" --workdir "$repository" \
  "$builder" sh -c '
    set -eu
    test "$(rustc -vV | sed -n "s/^host: //p")" = "$SENLUN_BUILD_TARGET"
    sh scripts/build-linux-service.sh
    linker_key="CARGO_TARGET_$(printf %s "$SENLUN_MUSL_TARGET" | tr "a-z-" "A-Z_")_LINKER"
    cc_key="CC_$(printf %s "$SENLUN_MUSL_TARGET" | tr - _)"
    # Debian musl-gcc specs inject an interpreter even into static PIE output.
    # Use Rust CRT objects and explicitly suppress that interpreter, retaining PIE.
    env "$linker_key=musl-gcc" "$cc_key=musl-gcc" \
      RUSTFLAGS="-C target-feature=+crt-static -C link-self-contained=yes -C link-arg=-Wl,--no-dynamic-linker" \
      cargo build --locked --release --target "$SENLUN_MUSL_TARGET" -p senlun-agent
    probe="target/$SENLUN_MUSL_TARGET/release/senlun-agent"
    if readelf --program-headers "$probe" | grep -q INTERP || readelf --dynamic "$probe" | grep -q NEEDED; then
      readelf --file-header --program-headers --dynamic "$probe" >&2
      echo "Agent must not require a host dynamic loader or shared libc" >&2; exit 1
    fi
    install -m 0755 "$probe" target/release/senlun-agent
  '

# The probe statically incorporates musl. Preserve the compiler sysroot's
# complete upstream and distribution copyright notice in the release archive.
docker run --rm --network none --read-only "$builder" \
  cat /usr/share/doc/musl/copyright > "$repository/target/release/MUSL_COPYRIGHT.txt"
test -s "$repository/target/release/MUSL_COPYRIGHT.txt"

# A successful link on a newer runner does not prove compatibility with users.
sh "$repository/scripts/verify-linux-service.sh" "$runtime" "$repository"
docker run --rm --network none --read-only --user 65532:65532 \
  --cap-drop ALL --memory 192m --pids-limit 64 \
  --mount "type=bind,src=$repository/target/release,dst=/opt/senlun,readonly" \
  "$runtime" /opt/senlun/senlun-agent --version

expected_agent_version=$(cat "$repository/crates/senlun-agent/version.txt")
test "$(docker run --rm --network none --read-only --user 65532:65532 \
  --cap-drop ALL --memory 192m --pids-limit 64 \
  --mount "type=bind,src=$repository/target/release,dst=/opt/senlun,readonly" \
  "$runtime" /opt/senlun/senlun-agent --version)" = "senlun-agent $expected_agent_version"

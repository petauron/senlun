#!/bin/sh
set -eu

# This runs inside the pinned native builder. Service-only changes here do not
# alter the separately invoked static Agent build.
cargo build --locked --release -p senlun-service

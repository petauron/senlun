#!/bin/sh
set -eu

# The caller supplies the already pinned runtime and repository path.
runtime=$1
repository=$2
docker run --rm --network none --read-only --user 65532:65532 \
  --cap-drop ALL --memory 192m --pids-limit 64 \
  --mount "type=bind,src=$repository/target/release,dst=/opt/senlun,readonly" \
  "$runtime" /opt/senlun/senlun-service --version

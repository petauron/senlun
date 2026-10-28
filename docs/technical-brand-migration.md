# Senulun to Senlun migration

Senlun is the canonical product, repository, executable and environment name.
The repository is renamed in place; published history, tags and existing signed
catalog records are not rewritten. Node IDs, saved Agent tokens, account/TOTP
records and traffic history remain unchanged. Browser cookies use the new name,
so administrators sign in again after the upgrade.

## Service

Stop the previous writer before changing its image or mount. Keep numeric
ownership 65532:65532 and the complete existing persistent data. The new image
mounts the retained data at `/var/lib/senlun`. Its explicit `migrate-and-serve`
command reads the previous `store/senulun.db` and its matched metrics database,
uses the existing SQLite paired backup operation, and atomically publishes
`senlun-store/senlun.db` and `senlun.metrics.db` with an identity receipt.
Missing or mismatched halves fail closed; never initialize a replacement store.
Original files and the matched backup remain recovery evidence, not live writers.
Retain external traffic encryption keys and configured file values.

## Agent and configuration

Convert private environment files with `deploy/migrate-environment.py` from
`SENULUN_*` to `SENLUN_*`. Duplicate assignments fail closed; values are not
interpreted or rewritten. Move only inspected owned paths and update their
explicit references. Rename the existing service account without changing its
numeric UID/GID. Keep the exact saved credential file and Service URL, stop the
old unit, then start `senlun-agent.service` with the verified new binary. Do not
run enrollment again or generate a replacement identity. The installer rejects
an unmigrated `/etc/senulun` configuration.

The source change alone does not migrate an installation. Catalog recipes and
Vastora's retained-resource handling must be updated before managed upgrades.
Historical signed identities and task records remain unchanged; runtime names
must agree with the actual ownership receipt before lifecycle operations resume.
Verify unchanged node IDs, authentication, retained history/traffic and fresh
reports before removing retired executable/unit paths. Keep the same public URL.

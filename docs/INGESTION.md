# Agent ingestion group commit

The Service collects snapshot and probe requests into bounded SQLite group
commits. Sampling intervals, source timestamps, traffic-period accounting and
the released HTTP request protocol are unchanged. Exact history now lives in
a ten-minute memory window; durable history uses minute summaries. See
[Monitoring storage](METRICS-STORAGE.md) for retention, migration and bounds.

## Bounds and acknowledgement

- At most 64 requests wait in the queue and 64 requests share a transaction.
  The existing 128-request HTTP concurrency limit still applies. Each request
  remains subject to the existing body and protocol limits.
- A batch waits at most one second to collect more requests. A full batch
  flushes immediately. The regular three-second Agent timer is unchanged.
- HTTP success is sent only after the ledger transaction commits. WAL and
  `synchronous=FULL` settings are unchanged; queue admission is not a durable
  acknowledgement for every exact historical sample. Hot samples and the
  unflushed current minute are volatile; committed traffic counters are not.
- Queue saturation, worker loss and acknowledgement timeouts return HTTP 503.
  An ambiguous timeout can still be followed by a commit, as with the previous
  blocking database path. Recent sample IDs are deduplicated across normal
  restart within the bounded window described in the storage document.
- Cancelling a caller does not remove its accepted request from the batch.
  Closing the queue drains accepted requests while the runtime remains alive.
  An abrupt process exit may lose exact hot history and the unflushed minute,
  including accepted samples; committed traffic accounting remains durable.

## Isolation and accounting

Authentication is checked again against the current database when processing
the request. A revoked credential cannot retain authority by waiting in a queue.
Each request has a savepoint: invalid credentials, rate-limit failures and invalid
probe batches do not discard valid neighbouring requests or leave partial probe
results. SQLite/commit failures roll back the whole batch before any success is
sent. Completed minutes flush transactionally; retention pruning is separate.

Every accepted unique sample contributes to its minute summary. Traffic counters are processed in queue
order using the existing per-direction deltas and reset rules, within the same
transaction as their samples. No latest-only replacement, averaging or dropped
samples is used to calculate traffic. Display history is aggregated; the
backed-up forward migration is described in the monitoring-storage document.

## Verification

Focused tests cover exact samples and traffic deltas, duplicate and too-frequent
reports, counter resets, invalid probe isolation, commit failure, acknowledgement
ordering, caller cancellation, credential revocation, queue draining and overload.
Run the Service tests and Clippy using the repository's documented toolchain.

For I/O comparisons use isolated synthetic data and identical binaries/settings
apart from this change. Include warm-up and WAL checkpoint/shutdown writes,
verify accepted/stored counts and traffic totals, and report acknowledgement
latency as well as write volume. Fresh-database comparisons with Komari do not
establish equal crash durability: Komari may acknowledge an in-memory queue,
whereas Senlun waits for the SQLite commit. Historical-store tests must compare
the same Senlun database pair and preserve both runtime durability settings.

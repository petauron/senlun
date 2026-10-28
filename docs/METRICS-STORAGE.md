# Monitoring storage

Senlun follows Komari's short exact window plus sealed-minute storage model.
The first implementation has two levels: exact samples for ten minutes in a
bounded in-memory SQLite database, and persisted UTC-minute summaries for the
configured history retention. `SENLUN_RETENTION_DAYS` is unchanged; this change
does not silently replace the existing retention with a new default.

The reference is Komari 1.6.0 at
[`1b76b7ad`](https://github.com/komari-monitor/komari/tree/1b76b7ad5bc3de6529bb7f0577069b4ac241fec3):
its [rollup implementation](https://github.com/komari-monitor/komari/blob/1b76b7ad5bc3de6529bb7f0577069b4ac241fec3/pkg/metric/rollup.go)
and [storage configuration](https://github.com/komari-monitor/komari/blob/1b76b7ad5bc3de6529bb7f0577069b4ac241fec3/internal/metricstore/config.go).
Senlun implements the hot/minute slice first, retains its existing retention
policy and durable traffic ledger, and preserves exact latency quantiles using
bounded counted distributions instead of introducing approximate sketches.

## Sampling, persistence and bounds

- Agent sampling and source timestamps are unchanged. Buckets use Service
  receive time, aligned to UTC `[minute, minute + 60 seconds)` boundaries.
- The hot database is capped at 64 MiB. Pending HTTP requests retain the existing
  queue/concurrency/body bounds. Database-full or queue-full conditions fail
  explicitly; samples are not silently dropped to reclaim memory.
- Completed minutes flush every three seconds and during ingestion. Clean
  shutdown also flushes the partial current minute. A backup flushes the hot
  window owned by its Storage instance; a standalone CLI cannot read another
  process's volatile memory and backs up that Service's persisted minutes.
- The traffic ledger remains independent and is committed with each ingestion
  batch before success is acknowledged. Its integer counters, per-direction
  reset behavior and configured billing/reset days are unchanged.
- Exact hot history and the unflushed current minute are volatile, as in Komari.
  An abrupt process exit can lose them even though the traffic ledger committed.
  Success therefore confirms the ledger and acceptance into the hot window,
  not durable storage of every exact historical sample. Persisted summaries
  retain `synchronous=FULL`; this change does not weaken durable database modes.
- Snapshots have a ceiling of 64 samples per node/minute, above the existing
  minimum receive spacing. Probes have a ceiling of 32 samples per node/task/
  minute; the existing minimum five-second interval and two-per-interval gate
  normally admit at most 24. The extra bound also covers repeated restarts.

## Statistics and deduplication

Snapshot summaries retain per-field count, sum, minimum and maximum, plus the
last record. Re-aggregation is weighted by counts; a minute's average is not
given the same weight as a minute with more samples. Cumulative network counter
displays use their last integer values. The traffic ledger never derives usage
from these display summaries.

Probe summaries retain successful/failed counts, latency totals, last success,
error counts, and a sorted counted latency distribution. Each contribution
holds at most 32 samples, even when every latency differs. This preserves
the existing exact interpolated p50/p95/p99 without adding a digest dependency
or reducing latency precision. It is not a serialized list of original probe
records: individual source timestamps and complete records are not retained.
Queries let SQLite merge and sort frequencies; application memory does not
accumulate all retained values into a per-task vector.

Recent sample IDs are retained for a bounded deduplication window through the
end of the minute plus ten minutes. A partial index expires those ID lists once;
IDs are not retained throughout long-term history. Normal restart/retry within
that window cannot count committed minutes twice. The last snapshot ID is also
part of durable node state. An unflushed probe lost in an abrupt exit was never
part of a persisted minute; retrying it does not duplicate persisted history.

Hot rows keep transactional persisted markers. A read uses exact rows only when
they cover the entire persisted minute. Otherwise it uses the persisted summary
plus unpersisted hot rows. This prevents double counting or gaps during partial
eviction, late arrivals, backup flushes and same-minute restart/continuation.
Old histories have minute resolution: a query cutting through a historical
minute includes that complete bucket. Coverage/resolution metadata and the
probe detail labels distinguish summaries from individual observations.

## Migration, cleanup and recovery

The forward-only control/metrics schema pair is **7 / 2**. Before converting a
released store, startup creates a private, matched backup of both databases and
their identity/version manifest. Conversion requires exclusive rollback journals
with FULL durability, aggregates old data in bounded batches, verifies sample
and failed counts, and commits both schema markers atomically. Source tables are
removed only within that transaction. Legacy minutes exceeding the new runtime
limits are split into bounded contributions (64 snapshots or 32 probes each),
without dropping samples. Reads merge their counts, statistics and frequencies.
Normal minute continuations coalesce with the last contribution when they fit;
no complete legacy bucket is loaded into an unbounded in-memory distribution.
Invalid source data, full databases and integrity failures fail closed. There is
no automatic downgrade.

History pruning runs separately from ingestion. Deletion of nodes/tasks also
removes both hot and persisted monitoring data. Deleted raw pages become reusable
SQLite free pages; conversion does not promise immediate physical file shrinkage.
Restore the matching pre-upgrade backup pair when recovery requires old software;
never mix a control database with an unrelated metrics file.

The implementation uses existing rusqlite, Tokio and serde dependencies. Compare
I/O using complete matched lifecycles, including fixed cleanup work and final
checkpoint/shutdown flushing. Report per-process and backing-device counters
separately, retain variation, and verify traffic totals independently.

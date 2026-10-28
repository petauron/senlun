# Minute storage validation — 2026-10-19

This is an isolated synthetic test of the minute-storage candidate before
integrating the latest main branch. The executable hashes identify the measured
code; the rebased PR is checked separately by CI. This is not a production
measurement or a current Komari performance comparison.

## Versions and conditions

- Baseline: batching commit `1f40767984048bc6476a95b4bb2b84b45f285113`.
- Baseline executable SHA-256:
  `c54b130f4b87e189ec9b98fbe86ca59bb84eccb9cd273607f669e315c099f193`.
- Candidate executable SHA-256:
  `0a747635236b72f5f69c0e778a21ca3420e8f162ae843f17fb3fdbaa1a8da214`.
- Both use Rust 1.98.0 release builds with the same minimal embedded Web stub.
  These measurements validate the Service/API/storage, not rendered Web assets.
- Unprivileged Debian 13 x86-64 LXC: two CPU cores, 2 GiB RAM, 512 MiB swap,
  8 GiB root disk. Existing project dependencies only; no production data/routes.
- Matched initial histories: 201,600 host samples plus 201,600 probe samples,
  including 40,320 failures, across ten nodes and three probe tasks per node.
  Host history spans about 16.8 hours; probe history about 9.3 hours. This is a
  representative populated fixture, not a full production seven-day database.
- Host-only load: 22 reports per node at three-second intervals, 220 requests.
  Probe-only load: 220 requests containing 660 probe results. Each run crosses
  two UTC minute buckets, then waits four seconds and shuts down normally.
- Two rounds per mode, in baseline/candidate then candidate/baseline order.
  All eight runs include startup, maintenance, shutdown and checkpoint writes.
  Fixture copies are fsynced before counters start; no host cache flushes or
  networking/storage changes are used. Idle alignment time varies by run, so
  elapsed run duration is not treated as a throughput result.

### Why some full cycles took about 100 seconds

After startup, the harness reads the current UTC-minute phase. If it is outside
seconds 5–35, it executes `sleep((5 - phase) % 60)` before sending the fixed load.
This explicit alignment wait can approach thirty seconds. Candidate host round
1 took 100.87 seconds versus 70.94 in round 2; candidate probe round 2 took
100.49 versus 70.94 in round 1. Their extra 29.93/29.55 seconds are consistent
with that wait. Baseline probe round 1 similarly took 85.80 versus 71.15 seconds.
The harness did not separately record the exact wait/shutdown durations, so these
differences are not precise shutdown timings.

Shutdown sends SIGTERM and waits up to fifteen seconds. Its forced-kill branch
raises an error before a result can be accepted/appended; all eight runs reached
their count checks and restart/read validation successfully. A killed or
unflushed run is not silently included as a low-write result. Both candidate
host runs and both probe runs preserve all 220/660 newly submitted samples
after shutdown and restart, with matched traffic/statistics checks.

## Steady write volume

Bytes below are the two individual runs. Reduction uses their arithmetic means.
Process writes use Linux child-process `ru_oublock × 512` over the complete
Service lifecycle. Backing writes use the isolated CT's cgroup counter on one
underlying block device, without summing device-mapper ancestors. The latter
includes CT background/filesystem work and is not SSD NAND write amplification.

| Load/counter | Baseline bytes, rounds 1 / 2 | Candidate bytes, rounds 1 / 2 | Reduction |
| --- | ---: | ---: | ---: |
| Host, process | 31,858,688 / 31,858,688 | 2,621,440 / 2,588,672 | 91.82% |
| Host, backing | 23,588,864 / 12,972,032 | 1,368,064 / 1,286,144 | 92.74% |
| Probe, process | 104,624,128 / 104,611,840 | 4,517,888 / 4,591,616 | 95.65% |
| Probe, backing | 35,082,240 / 35,053,568 | 1,359,872 / 1,343,488 | 96.15% |

All 1,760 ingestion requests succeeded. Per-run median acknowledgement latency
was 570–596 ms for the baseline and 559–562 ms for the candidate; p99 was
1,017–1,063 ms and 1,006–1,012 ms respectively. The existing one-second maximum
batch collection wait remains in effect.

After each measured shutdown the same service is restarted for twenty public
history API checks. All 160 checks passed. Per-run median read latency was
90.53–93.43 ms before and 57.91–61.58 ms after. Historical counts, host CPU
sum/min/max, probe counts/failures, latency sums and traffic digests match across
matched runs. Probe-only runs leave the traffic ledger unchanged. Actual
p50/p95/p99 differences were zero; the floating-point comparison guard was
predeclared as 1e-10 ms for quantiles and 1e-6 for summed latencies.

## Migration and space

The forward migration, including its paired backup, took 15.169 seconds and
accounted for 431,431,680 process-write bytes and 416,817,152 backing-write
bytes. This one-time cost is excluded from steady-state savings.

| Data | Raw rows | Minute contributions | Active pages before / after, including indexes |
| --- | ---: | ---: | ---: |
| Host | 201,600 | 10,100 | 79,958,016 / 42,225,664 bytes |
| Probe | 201,600 | 16,830 | 78,991,360 / 14,364,672 bytes |

Combined active page usage falls **64.40%**. Physical database files do not
immediately shrink: 159,166,464 bytes before and 160,088,064 after; released pages
are reusable. No VACUUM or automatic destructive shrinking is part of migration.

Probe JSON averages 524 bytes, with a 632-byte maximum in the ordinary fixture.
A separate 32-distinct-RTT restart case uses 2,048 bytes while retaining recent
deduplication IDs and 801 bytes after those IDs expire. Larger legitimate old
minutes split into bounded contributions rather than failing or losing values.

## Correctness and limits

- Service library suite: **133 passed, zero failed**, 47.18 seconds.
- Strict all-target Service Clippy: passed with `-D warnings`.
- Regression cases include 8,194 same-minute old samples across multiple flush
  batches, bounded contributions, exact quantiles/peaks, restart retry of an ID
  in an earlier contribution, weighted comparisons, invalid-data migration
  rollback, backup retention, hot eviction and failed commits.
- Recent exact samples and the unflushed minute are volatile, as in Komari.
  Abrupt exits may lose them; the independently committed traffic ledger retains
  its existing durability. Clean shutdown persists the partial minute.
- These results compare two Senlun implementations under fixed synthetic load.
  They do not establish production savings, memory superiority over Komari,
  identical cross-product durability, or rendered frontend acceptance.

Implementation and recovery policy: [Monitoring storage](METRICS-STORAGE.md).
No release or production deployment was part of the measurement. The isolated
test container was shut down afterward, with its evidence retained.

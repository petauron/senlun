# Correct current-cycle local traffic

In **Admin → Nodes**, select a node and choose **校正本周期本地流量**.
Enter upload and download separately, choose GB (1000³ bytes) or GiB (1024³
bytes), provide a reason, preview, then confirm. The saved billing direction
and UTC reset day apply. A combined bill must not be copied into both fields.

This changes only the current local cycle's accumulated upload/download. It
does not change Agent raw counters, historical samples, quota, reset day,
provider credentials or external traffic snapshots. Subsequent report deltas
continue to accumulate. Sub-byte input is rounded to the nearest byte.

## Administrator API

`GET /api/admin/nodes/{id}/traffic-correction` returns the current cycle,
last sample time, billing direction, quota and a baseline. All byte counts and
the audit cursor are decimal strings so JavaScript does not lose integer
precision. The browser does not supply or edit the Agent's raw counters.

`POST` to the same URL accepts:

```json
{
  "baseline": "the complete baseline object returned by GET, unchanged",
  "used_up_bytes": "1000000000",
  "used_down_bytes": "2000000000",
  "reason": "Correct an imported starting value"
}
```

The baseline above is explanatory: the actual request contains the returned
JSON object, not a string. Existing administrator authentication, exact Origin
and CSRF checks protect writes. The operation actor comes from the server's
authenticated session, never the request body.

The existing storage transaction and correction calculation are reused.
Normal reports after the preview are added to the target. Node mutations after
the preview (another correction, reset or metadata change), a different cycle,
raw-counter discontinuity, missing evidence or an older-than-two-minute baseline
reject the write with HTTP 409. The latest node sample must also be fresh under
the configured offline threshold. No new ledger, schema or generic request
deduplication framework is introduced.

Successful writes record administrator, node, cycle, old/new values, sample time
and reason in the existing bounded audit history. A repeated write returns 409,
not another addition or audit event. If a response times out, refresh and inspect
the stored values before forming a new preview; the request may have committed.
Never automatically retry against a newly fetched baseline.

## Local verification status (2026-10-18)

- Rust 1.98.0: traffic tests 13 passed; control tests 20 passed; authentication
  tests 14 passed (these selections overlap). Formatting and `git diff --check`
  passed.
- Existing Rust 1.98.1 Clippy: workspace/all-targets with warnings denied passed.
  The pinned 1.98.0 installation lacks Clippy; this is not an exact-toolchain
  Clippy result.
- TypeScript 5.9.3 checked the standalone traffic conversion utility; Node
  runtime assertions covered unit conversion, exact large-integer round trips,
  invalid input, rounding and billing direction.
- Frontend acceptance remains blocked: official npm registry TLS connections
  fail and the offline cache lacks locked `zwitch@2.0.4`. Keep the official
  registry and lockfile unchanged. Whole-app Vue type checking, Vitest,
  frontend build and browser tests have **not passed or been accepted**.
- Backend tests used a clearly labelled temporary static embed fixture, not a
  product frontend build. It was removed after backend verification. All tests
  used isolated synthetic data, not production services or databases.

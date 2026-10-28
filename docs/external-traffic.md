# External traffic sources

Senlun can keep a provider-reported traffic snapshot next to, but never merged with, the Agent's local interface counters. This is an administrator-enabled HTTP integration, not a provider-specific adapter or a remote command channel. Each node can have one source; at most 32 sources may be configured. Enabled sources are polled by the Service roughly every 30 minutes, eight at a time, without changing any Agent.

## Prepare credential encryption

Before saving a source, create a separate file of exactly 32 random bytes with mode `0600`, for example `openssl rand -out /var/lib/senlun/traffic-credential.key 32` followed by `chmod 600 /var/lib/senlun/traffic-credential.key`. Set `SENLUN_TRAFFIC_KEY_FILE` to its absolute path and restart the Service. Keep this file with the matched Service database backup. Losing it prevents decryption of stored source credentials; a restored database with configured sources fails closed when the key is absent. Do not put the key, provider credentials, or full subscription URLs in Git, CI logs, screenshots, or support tickets.

In the existing administrator page, open **External traffic** and select a node. Configure a source name, the HTTPS GET URL that returns traffic JSON, and JSON Pointer paths for the raw integer-byte fields. The up and down fields are required. An optional `used` pointer takes precedence over adding those fields; use it only when the provider explicitly reports a separate billable total. An optional quota pointer can be expressed in bytes, decimal GB, or binary GiB. An optional reset-time pointer displays the provider's value without inventing a billing cycle or timezone.

Authentication modes are none, fixed Bearer token, and JSON login with refresh-token rotation. The latter takes HTTPS login and refresh URLs on the *same origin* as the traffic URL; POST field names and response JSON Pointers are configurable. Credentials stay encrypted at rest in the Service control database and are never sent to an Agent or returned by the admin API. Leave credential inputs blank when editing to retain the stored values. A provider requiring CAPTCHA, MFA, or other interactive approval cannot be bypassed; a failed login leaves the last successful snapshot visible but stale. No browser Cookie is copied into the Service. A rejected access token permits refresh and, only if refresh is explicitly rejected with HTTP 401/403, one password-login fallback. Refresh timeouts, rate limits, and provider errors do not trigger password login. Authentication failures (including refresh responses without access tokens or with invalid JSON, and interactive login responses without tokens) pause automatic polling until the administrator reviews the account or verification requirement and saves the source again. Saving resumes polling without changing local counters; external snapshots remain stale until a successful sync.

Only public HTTPS destinations are accepted at request time, including after DNS resolution. Redirects and HTTP proxies are disabled; responses are capped at 128 KiB and requests time out after 10 seconds. The source sends only login, refresh, and GET traffic requests. Provider response bodies, including unrelated secrets they may contain, are never persisted or logged. Snapshot status is explicit; a failed sync does not replace usage with zero. This is a self-maintained integration, so a provider API change may require reconfiguration.

## Example mapping

For an API whose GET response has `data.resource_data.net_in`, `net_out`, and `traffic` as byte counters, and `data.config.traffic` as an allowance in GiB, set the down, up, used, and limit pointers respectively to `/data/resource_data/net_in`, `/data/resource_data/net_out`, `/data/resource_data/traffic`, and `/data/config/traffic`; set the limit unit to `gib`. Check the provider's field semantics before saving: runtime usage, billable usage, and traffic since the last reset may be different measurements. Keep the source disabled until those mappings have been checked against the provider dashboard.

## Selected quota source (issue #70 MVP)

Each node's existing traffic settings include **local / provider**. Existing nodes
remain local by default. This one choice controls the card, list and traffic-quota
alert percentage through the same Service read model; live status updates carry
that selection. Local raw counters, period counts, history and the separate local
and provider detail views remain unchanged. The overview aggregate remains local.

Provider mode uses only the snapshot's billable used bytes and quota. It never
borrows the local quota, direction or reset day. Missing or zero provider quota is
unknown, not unlimited. Disabled, unconfigured, authentication-required, failed,
missing and over-90-minute-old syncs have explicit states. Last successful values
remain visible where available, but their percentage is unavailable to alerts;
there is no automatic fallback to local. Queued traffic notifications are cancelled
when data becomes unavailable; already dispatched requests cannot be recalled.
Unknown values cannot resolve a firing
incident, and pending duration must start again after usable data returns.

Changing the chosen source or editing/deleting its configured resource ends the
previous traffic evaluation without sending recovery, preserving its record and
monitoring history. A later breach starts a fresh duration. Already dispatched
webhook requests cannot be recalled. Existing authentication pause/recovery rules
above still apply.

A reset-time string is only provider metadata: this MVP cannot verify a complete
billing cycle or identify an old-cycle response solely from HTTP success. The UI
discloses that the provider cycle is unknown. Administrators must verify that the
configured fields represent the same current bill before selecting provider mode.

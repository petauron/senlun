# Production operations

Senlun has two deployment roles: run `senlun-service` on the monitoring host, and run one outbound-only `senlun-agent` on every monitored Linux host. The Agent never accepts inbound commands. Run both under dedicated unprivileged accounts.

## Trust boundary

Alpha.3 includes administrator authentication and defaults to a private dashboard. Until the first administrator is created, all monitoring read APIs fail closed; afterward, only an administrator can deliberately enable public reads. Administrative mutations always require an authenticated session, exact Origin and CSRF protection. Configure the HTTPS browser origin and private bootstrap token before upgrading; see [AUTH.md](AUTH.md). Older Alpha.2 artifacts require external dashboard authentication. Keep the default loopback bind and an HTTPS reverse proxy; do not expose port 8080 directly to the Internet.

Remote Agents accept only HTTPS Service URLs. Senlun has no analytics, telemetry, runtime CDN, or mandatory third-party account. Metrics go only to the configured Service.

The visitor-information pill obtains the current visitor's public address from
`/api/me`. A reverse proxy hides the browser's transport address, so this field
stays unavailable by default. To enable it, set `SENLUN_TRUSTED_PROXY_IP` to
the **exact IP of the proxy's connection to Senlun** and configure that proxy
to discard any client-provided `X-Real-IP` and overwrite it with the verified
client address. Senlun accepts a single public IP in that header only from
the exact configured peer; it does not trust `X-Forwarded-For` or
`CF-Connecting-IP` from arbitrary callers. Keep the Service on a private or
loopback listener and verify the proxy's overwrite behavior before enabling
this option. No external IP service is queried by this feature.

**Default outbound request:** Agents with no manual region use GeoJS to discover
their country. GeoJS sees the public egress IP and Agent version, not Senlun tokens
or metrics. To prevent this, set `SENLUN_GEOIP_PROVIDER=disabled` in the Agent
environment **before starting or upgrading**. See [node location](#node-location-and-globe-placement).

## Verify a release

Download the archive for the host architecture together with `SHA256SUMS`, its SPDX SBOM, and the GitHub artifact attestation. Verify before extracting:

```bash
sha256sum --check --ignore-missing SHA256SUMS
gh attestation verify "$ARCHIVE" --repo petauron/senlun
```

Set `ARCHIVE` to the downloaded `senlun-<tag>-linux-<arch>.tar.gz` from a release
built in `petauron/senlun`. This policy is for releases after the repository
rename. Historical artifacts were signed by `petauron/senlun`; retain their
original provenance and verification policy instead of treating them as newly
signed Senlun releases. Published images at `ghcr.io/petauron/senlun` remain
available under their original immutable digests. Do not overwrite old releases
or relax the new-source attestation policy to accept both identities.

Official Linux archives target `x86_64` and `aarch64` GNU/Linux with a Debian 12 (glibc 2.36) runtime baseline. Both binaries are built in the pinned Debian 12 Rust image and must start in a clean Debian 12 runtime before packaging. The same build/verification script runs in CI. Use the container or build from source for other environments, including musl-based distributions.

The release workflow validates binaries, SBOMs, and a multi-architecture container before creating a hidden draft. It then pushes and attests the image and publishes the GitHub Release only as the final step. If final publication fails or the run is cancelled, the workflow attempts to remove the GHCR package version carrying that release tag and leaves the draft for a safe rerun. Cleanup is limited to two minutes. If cleanup fails, times out, or the runner is forcibly terminated, the maintainer must inspect and remove that tagged GHCR package version before retrying; never publish the draft manually while cleanup is unresolved.

## systemd installation

The release archive contains the binaries and `deploy/`. Review the environment example, then install from the extracted, verified archive:

```bash
sudo ./deploy/install-systemd.sh service "$PWD/senlun-service"
sudoedit /etc/senlun/service.env
sudo systemctl start senlun-service
```

The installer creates a dedicated account and mode-0700 state directory, records the previous binary, installs a hardened unit, and enables it without starting an unconfigured first installation. An update restarts the unit only if it was already running.

Before a new or incomplete installation, it rejects the selected component's retained Pulse or Senulun configuration, state directories and units, including dangling links and old managed Agent units. Complete the documented offline brand migration first. A validated Senlun release link with an executable target and existing configuration can still be upgraded while old recovery resources are retained; this check does not copy, delete or migrate those resources.

Create an enrollment token locally against the Service database. The JSON response contains an ID, the only display of the token, and its expiry time:

```bash
sudo -u senlun env SENLUN_DATABASE_PATH=/var/lib/senlun/store/senlun.db \
  /usr/local/bin/senlun-service enrollment create --ttl-seconds 600
```

On the Agent host:

```bash
sudo ./deploy/install-systemd.sh agent "$PWD/senlun-agent"
sudoedit /etc/senlun/agent.env
sudo install -o senlun-agent -g senlun-agent -m 0600 /path/to/securely-transferred-token \
  /etc/senlun/agent-enrollment-token
sudo systemctl start senlun-agent
```

The token file must contain only the token value. It is read only when the Agent has no stored credential. `systemctl start` does not wait for enrollment. Verify that `journalctl -u senlun-agent` reports `agent credentials saved` and that this node appears online before deleting the token:

```bash
sudo test -s /var/lib/senlun-agent/agent-credentials.json && \
  sudo rm -f /etc/senlun/agent-enrollment-token
```

## Node location and globe placement

The globe groups nodes by country/territory, not by group names or individual
city coordinates. Set `SENLUN_NODE_REGION=US` (or another two-letter country code)
for manual placement. A nonempty manual region takes precedence and prevents all
GeoIP requests, even if a provider is configured.

Country discovery through GeoJS is enabled by default when no manual region is
set. Upgrade **each Agent binary** to a build containing this feature. The equivalent
explicit configuration in `/etc/senlun/agent.env` is:

```ini
SENLUN_NODE_REGION=
SENLUN_GEOIP_PROVIDER=geojs
```

Alternatively select `ipinfo`. The only accepted provider values are `disabled`,
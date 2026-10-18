# Standalone installation

Senulun has no runtime dependency on Vastora or its catalog. Its Service owns
the dashboard, administrator accounts, themes, settings and SQLite data. Each
Agent connects outbound to the configured Service and keeps its own node ID
and credential. A package manager may install binaries, supervise processes,
preserve application data, upgrade a version and publish an access entry.

## Install the Service

Download the Linux archive for your architecture from a
[published Senulun release](https://github.com/petauron/senulun/releases), together
with its `SHA256SUMS` and provenance. Follow the verification procedure in
[operations](OPERATIONS.md#verify-a-release) before extracting it. The native
binaries require glibc 2.36 or newer.

From the extracted release directory:

```sh
sudo ./deploy/install-systemd.sh service "$PWD/senulun-service"
sudoedit /etc/senulun/service.env
sudo systemctl start senulun-service
```

Set `SENULUN_PUBLIC_URL` to the exact browser-facing HTTPS origin and put your
HTTPS reverse proxy in front of the default `127.0.0.1:8080` listener. Keep the
uninitialized instance private until you finish administrator setup at `/login`.
Senulun creates and authenticates this account itself. There is no external
account or package-manager initialization token.

Alternatively, set `SENULUN_SERVICE_IMAGE` to the release's immutable
`ghcr.io/petauron/senulun@sha256:…` image reference and `SENULUN_PUBLIC_URL` to
your HTTPS origin, then run the bundled Compose file:

```sh
docker compose -f deploy/docker-compose.yml up -d
```

The Compose service binds its host port to loopback and persists data in
`senulun_data`. Configure HTTPS using your own reverse proxy. The browser origin
and the Agent's Service URL can differ, provided both reach the same Service.
Agents need an HTTPS endpoint reachable without an interactive browser login.

## Enroll an Agent

In Senulun's administrator area, create a short-lived, single-use enrollment
token. Install the Agent from the verified release on the monitored host:

```sh
sudo ./deploy/install-systemd.sh agent "$PWD/senulun-agent"
sudoedit /etc/senulun/agent.env
```

Set `SENULUN_SERVICE_URL` to the reachable HTTPS Service URL, ending in `/`,
and choose `SENULUN_NODE_NAME` and an optional `SENULUN_NODE_GROUP`. Transfer
only the token value to a protected file, then install it and start the Agent:

```sh
sudo install -o senulun-agent -g senulun-agent -m 0600 /path/to/token \
  /etc/senulun/agent-enrollment-token
sudo systemctl start senulun-agent
```

Without a manual region, GeoJS country discovery is enabled by default. It sees
the Agent's public egress IP and version, never credentials or metrics. Set
`SENULUN_GEOIP_PROVIDER=disabled` before starting or upgrading to opt out;
an explicit country code also avoids discovery requests.

Confirm the node is online in Senulun and that
`/var/lib/senulun-agent/agent-credentials.json` exists before deleting the
one-time token file. Keep this credential file during upgrades; enrolling again
would create a different monitoring identity.

The same inputs work when Vastora installs the Agent. Create the token in
Senulun, then provide its URL, node name, group and token in the package's
installation form. The Service may be hosted outside Vastora.

## Configuration, upgrades and removal

Use Senulun's own administrator area for nodes, probes, traffic, themes and
account settings. Native process settings live in `/etc/senulun/*.env`;
container settings are ordinary environment variables in your deployment.
`SENULUN_TRUSTED_PROXY_IP` is optional and must identify the exact proxy peer
that overwrites `X-Real-IP` with a verified visitor address. An empty setting
leaves forwarded visitor IPs disabled. See [the trust boundary](OPERATIONS.md#trust-boundary).

For a native upgrade, verify the new release and rerun its installer for the
same component. It preserves the environment and data and restarts an already
active unit. For Docker, preserve the same volume and change only the verified
image reference. Follow the application backup and upgrade procedure in
[operations](OPERATIONS.md); database migrations are forward-only, and program
rollback requires a matching data backup.

Uninstalling the program and deleting its data are separate operations. Preserve
both Service databases and its themes/credential keys, or the Agent identity
file, until you deliberately choose to discard them. Package ownership names
such as an existing `vastora-pulse` container do not affect Senulun's protocol
or require another package manager to run it.

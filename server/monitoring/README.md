# Fluxkit Grafana dashboard

[`fluxkit-server.json`](fluxkit-server.json) is an importable Grafana dashboard for the
server's Prometheus endpoint. It uses Grafana's built-in Prometheus data source and standard
Stat and Time series panels. It does not change the server configuration or install services.

## Connect Prometheus

The data flow is Fluxkit `/metrics` → Prometheus → Grafana. Grafana's data source URL must
point to Prometheus, not to Fluxkit's `/metrics` endpoint.

Add the following job to the existing Prometheus configuration, replacing the example host:

```yaml
scrape_configs:
  - job_name: fluxkit
    scrape_interval: 30s
    scrape_timeout: 10s
    metrics_path: /metrics
    scheme: https
    static_configs:
      - targets: ['fluxkit.example.org']
```

Merge this entry into an existing `scrape_configs` list rather than creating a second key.
Reload Prometheus using the operator's existing procedure. Check its Targets page: the
`fluxkit` target should be UP. Query `fluxkit_accounts` to confirm usage metrics are present.
Scrape a specific server instance rather than a load-balanced address; counters and active
user windows belong to individual processes.

The endpoint is enabled by default and does not require a Fluxkit account or bearer token.
Allow the Prometheus host through the reverse proxy's `/metrics` restriction; see
[server deployment](../README.md#metrics). Keep TLS verification enabled. The example assumes
the proxy serves Fluxkit at the root of the host; adjust `metrics_path` for a path prefix.

| Server setting             | Default | Behavior                                                          |
| -------------------------- | ------- | ----------------------------------------------------------------- |
| `Metrics__Enabled`         | `true`  | `false` removes the endpoint; instrumentation still runs.         |
| `Metrics__SnapshotSeconds` | `15`    | Cache lifetime for usage database counts, from 0 to 3600 seconds. |

These are environment variable spellings; JSON configuration uses `Metrics:Enabled` and
`Metrics:SnapshotSeconds`. Counts refresh during a scrape once the cache has expired, not on
a background timer. A shorter scrape interval does not force a fresh database query on every
scrape. With the example, the first scrape after expiry normally refreshes the counts every
30 seconds.

## Import into Grafana

1. Configure a Prometheus data source in Grafana and confirm its connection succeeds.
2. Open **Dashboards → New → Import**, upload `fluxkit-server.json`, and import it.
3. At the top of the dashboard, select **Prometheus**, **Job**, and **Instance**.
4. In the Prometheus data source settings, set the scrape interval to match the job
   (`30s` for the example). The rate panels use `$__rate_interval` for their lookback window.
5. Allow several scrapes before expecting rates and latency percentiles to appear.

The data source is a dashboard variable, so no data source UID needs editing in the JSON.
Job and Instance each select exactly one value. To compare servers, use separate dashboard
tabs or copies. If separate deployments share a job, select the desired instance explicitly.

## Included views

- Overview: scrape status, active accounts, pending approvals, projects, stored requests,
  and distinct active users in the last five minutes.
- Inventory: account status, project kinds, project rows, variable classifications, stored
  value scopes, active server administrators, team memberships, and configured account vaults.
- Activity: sign-in results and modes, registrations, change batch outcomes, applied row
  operations, value write outcomes, feed read results, project creation/deletion, and account
  administration. All nine Fluxkit event counters are covered as rates per second.
- Health: API request rates by status and route, estimated p50/p95/p99 latency, all HTTP
  requests in progress, process CPU and resident memory, garbage collections, database
  snapshot query duration, and Prometheus scrape duration.

API traffic and latency panels select `endpoint=~"/api/.*"` to exclude health checks and
metrics scrapes. Routes use the existing template labels, without concrete project IDs.
The in-progress HTTP gauge includes all traffic. CPU is measured in CPU cores: `1` means
one logical core fully used, rather than one percent.

## Interpretation and current limits

- **Inventory is database-wide.** Two processes using the same database report the same
  account/project/value counts. Summing across them would double-count inventory. The
  dashboard deliberately selects one instance.
- **Activity is process-local.** Active users are distinct authenticated accounts seen by
  that process in overlapping `5m`, `1h`, and `24h` windows since startup. Background desktop
  polling counts as activity. These are neither open session counts nor durable daily
  active user totals, and cannot be summed into globally distinct users across replicas.
- **Counters restart with the process.** `rate()` handles counter resets. To inspect event
  totals over the selected dashboard range in Explore, use, for example:
  `sum by (result) (increase(fluxkit_change_batches_total{job="$job",instance="$instance"}[$__range]))`.
  Such totals are extrapolated estimates and can be fractional. Events before the first
  observed sample cannot be recovered by Prometheus.
- **Missing event series are possible.** Most counter label combinations only appear after
  their first event. An unused feature can show No data. Panels do not replace missing data
  with zero, which could conceal failed scrapes. Check Scrape status and the raw metric in
  Explore. No traffic also produces undefined latency percentiles.
- **Application counters cover their instrumented paths.** Requests rejected before the
  handler, including authentication failures and rate limits, may not increment a Fluxkit
  counter. The HTTP status panels provide the broader API view. Successful token refreshes
  are not fresh sign-ins.
- **UP does not mean fresh database counts.** Snapshot query failures retain previous
  values and still allow metric export. Before the first successful snapshot, gauges can
  be zero or absent. `fluxkit_usage_snapshot_duration_seconds` is the duration of the last
  successful refresh, not its age. No snapshot success/age/error metric currently exists,
  so these metrics alone cannot establish freshness. Check `/health/ready` separately for
  database connectivity; it is not scraped by this job.
- **Metric scope stays aggregate.** The dashboard queries counts and route templates. It
  does not query credentials, request payloads, project names, or user identities.

The JSON and queries should be checked against the actual Grafana/Prometheus versions and a
live target after import. Runtime meter names beyond the stable `process_*`,
`dotnet_collection_count_total`, HTTP, and Fluxkit metrics used here can vary with .NET and
library versions; additional ASP.NET Core/Npgsql panels should use names observed in the
deployment's scrape output.

References: [Grafana dashboard import](https://grafana.com/docs/grafana/latest/visualizations/dashboards/build-dashboards/import-dashboards/),
[Prometheus variables and rate intervals](https://grafana.com/docs/grafana/latest/datasources/prometheus/template-variables/),
and [Prometheus query functions](https://prometheus.io/docs/prometheus/latest/querying/functions/).

using System.Collections.Concurrent;
using System.Diagnostics;
using Microsoft.EntityFrameworkCore;
using Prometheus;

namespace Fluxkit.Server;

/// Prometheus metrics of this server: runtime and HTTP metrics plus aggregated usage statistics.
/// Labels never carry user names, project names, identifiers or request content.
public sealed class UsageMetrics : IDisposable
{
    private static readonly (string Label, TimeSpan Window)[] ActivityWindows =
    [
        ("5m", TimeSpan.FromMinutes(5)),
        ("1h", TimeSpan.FromHours(1)),
        ("24h", TimeSpan.FromHours(24))
    ];

    private readonly IServiceScopeFactory scopes;
    private readonly TimeSpan snapshotLifetime;
    private readonly IDisposable meters;
    private readonly ConcurrentDictionary<Guid, long> lastSeen = new();
    private readonly SemaphoreSlim refreshing = new(1, 1);
    private long snapshotTaken = long.MinValue;

    private readonly Counter logins;
    private readonly Counter registrations;
    private readonly Counter batches;
    private readonly Counter operations;
    private readonly Counter valueWrites;
    private readonly Counter feedReads;
    private readonly Counter projectsCreated;
    private readonly Counter projectsDeleted;
    private readonly Counter accountChanges;
    private readonly Gauge accounts;
    private readonly Gauge serverAdmins;
    private readonly Gauge projects;
    private readonly Gauge memberships;
    private readonly Gauge entities;
    private readonly Gauge variables;
    private readonly Gauge values;
    private readonly Gauge vaults;
    private readonly Gauge activeUsers;
    private readonly Gauge snapshotSeconds;

    public CollectorRegistry Registry { get; } = Metrics.NewCustomRegistry();

    public UsageMetrics(IServiceScopeFactory scopes, IConfiguration configuration)
    {
        this.scopes = scopes;
        var seconds = configuration.GetValue("Metrics:SnapshotSeconds", 15);
        if (seconds is < 0 or > 3600)
        {
            throw new InvalidOperationException("Metrics:SnapshotSeconds must be between 0 and 3600.");
        }
        snapshotLifetime = TimeSpan.FromSeconds(seconds);
        // Process and .NET runtime metrics, plus the .NET meters of ASP.NET Core, Kestrel,
        // the runtime and Npgsql (connections, command durations).
        DotNetStats.Register(Registry);
        var factory = Metrics.WithCustomRegistry(Registry);
        meters = MeterAdapter.StartListening(new MeterAdapterOptions
        {
            Registry = Registry,
            MetricFactory = factory,
            // Runtime-internal diagnostics meters (QUIC etc.) are noise for operators.
            InstrumentFilterPredicate = instrument =>
                !instrument.Meter.Name.StartsWith("Private.InternalDiagnostics", StringComparison.Ordinal)
        });

        logins = factory.CreateCounter("fluxkit_logins_total",
            "Sign-in attempts by mode and result (success, rejected, pending).",
            new CounterConfiguration { LabelNames = ["mode", "result"] });
        registrations = factory.CreateCounter("fluxkit_registrations_total",
            "New accounts awaiting approval, by sign-in mode.",
            new CounterConfiguration { LabelNames = ["mode"] });
        batches = factory.CreateCounter("fluxkit_change_batches_total",
            "Project change batches by result (applied, conflict, rejected).",
            new CounterConfiguration { LabelNames = ["result"] });
        operations = factory.CreateCounter("fluxkit_change_operations_total",
            "Applied project row operations by entity kind and operation (put, delete).",
            new CounterConfiguration { LabelNames = ["kind", "operation"] });
        valueWrites = factory.CreateCounter("fluxkit_value_writes_total",
            "Variable value writes by scope (shared, vault), operation (put, delete) and result.",
            new CounterConfiguration { LabelNames = ["scope", "operation", "result"] });
        feedReads = factory.CreateCounter("fluxkit_feed_reads_total",
            "Change feed reads by desktop clients by result (changes, unchanged, reload).",
            new CounterConfiguration { LabelNames = ["result"] });
        projectsCreated = factory.CreateCounter("fluxkit_projects_created_total",
            "Projects created, by kind and source (blank, upload).",
            new CounterConfiguration { LabelNames = ["kind", "source"] });
        projectsDeleted = factory.CreateCounter("fluxkit_projects_deleted_total",
            "Empty projects deleted by server administrators.");
        accountChanges = factory.CreateCounter("fluxkit_account_changes_total",
            "Administrative account changes (approved, disabled, reactivated, deleted).",
            new CounterConfiguration { LabelNames = ["change"] });

        accounts = factory.CreateGauge("fluxkit_accounts", "Accounts by status.",
            new GaugeConfiguration { LabelNames = ["status"] });
        serverAdmins = factory.CreateGauge("fluxkit_server_admins", "Active server administrators.");
        projects = factory.CreateGauge("fluxkit_projects", "Projects by kind.",
            new GaugeConfiguration { LabelNames = ["kind"] });
        memberships = factory.CreateGauge("fluxkit_team_memberships", "Team project memberships.");
        entities = factory.CreateGauge("fluxkit_project_rows", "Stored project rows by kind.",
            new GaugeConfiguration { LabelNames = ["kind"] });
        variables = factory.CreateGauge("fluxkit_variables", "Variable definitions by classification.",
            new GaugeConfiguration { LabelNames = ["classification"] });
        values = factory.CreateGauge("fluxkit_stored_values", "Stored variable values by scope.",
            new GaugeConfiguration { LabelNames = ["scope"] });
        vaults = factory.CreateGauge("fluxkit_account_vaults", "Accounts with a configured secret vault.");
        activeUsers = factory.CreateGauge("fluxkit_active_users",
            "Distinct accounts with an authenticated API request within the window (since server start).",
            new GaugeConfiguration { LabelNames = ["window"] });
        snapshotSeconds = factory.CreateGauge("fluxkit_usage_snapshot_duration_seconds",
            "Time taken by the last database query for the usage statistics.");

        // Pre-create label combinations so dashboards see zeros instead of missing series.
        foreach (var status in new[] { "pending", "active", "disabled" }) { accounts.WithLabels(status); }
        foreach (var kind in new[] { "personal", "team" }) { projects.WithLabels(kind); }
        foreach (var result in new[] { "applied", "conflict", "rejected" }) { batches.WithLabels(result); }
        foreach (var (label, _) in ActivityWindows) { activeUsers.WithLabels(label); }

        Registry.AddBeforeCollectCallback(async cancel =>
        {
            UpdateActiveUsers();
            await RefreshSnapshotAsync(cancel);
        });
    }

    public void Login(string mode, string result) => logins.WithLabels(mode, result).Inc();
    public void Registration(string mode) => registrations.WithLabels(mode).Inc();
    public void Batch(string result) => batches.WithLabels(result).Inc();
    public void Operation(string kind, string operation) => operations.WithLabels(kind, operation).Inc();
    public void ValueWrite(string scope, string operation, string result) =>
        valueWrites.WithLabels(scope, operation, result).Inc();
    public void FeedRead(string result) => feedReads.WithLabels(result).Inc();
    public void ProjectCreated(string kind, string source) => projectsCreated.WithLabels(kind, source).Inc();
    public void ProjectDeleted() => projectsDeleted.Inc();
    public void AccountChange(string change) => accountChanges.WithLabels(change).Inc();

    /// Records that an account made an authenticated request now.
    public void Seen(Guid user) => lastSeen[user] = Stopwatch.GetTimestamp();

    private void UpdateActiveUsers()
    {
        var now = Stopwatch.GetTimestamp();
        var ages = new List<TimeSpan>(lastSeen.Count);
        foreach (var (user, seen) in lastSeen)
        {
            var age = Stopwatch.GetElapsedTime(seen, now);
            if (age > ActivityWindows[^1].Window)
            {
                lastSeen.TryRemove(new KeyValuePair<Guid, long>(user, seen));
            }
            else
            {
                ages.Add(age);
            }
        }
        foreach (var (label, window) in ActivityWindows)
        {
            activeUsers.WithLabels(label).Set(ages.Count(age => age <= window));
        }
    }

    /// Database counts, refreshed at most every `Metrics:SnapshotSeconds` (default 15) however
    /// often Prometheus scrapes.
    private async Task RefreshSnapshotAsync(CancellationToken cancel)
    {
        var taken = Interlocked.Read(ref snapshotTaken);
        if ((taken != long.MinValue && Stopwatch.GetElapsedTime(taken) < snapshotLifetime)
            || !await refreshing.WaitAsync(0, cancel))
        {
            return;
        }
        try
        {
            var started = Stopwatch.GetTimestamp();
            using var scope = scopes.CreateScope();
            var db = scope.ServiceProvider.GetRequiredService<FluxkitDb>();
            var byStatus = await db.Users.AsNoTracking().GroupBy(user => user.Status)
                .Select(group => new { group.Key, Count = group.Count() }).ToListAsync(cancel);
            foreach (var status in new[] { "pending", "active", "disabled" })
            {
                accounts.WithLabels(status).Set(byStatus.FirstOrDefault(item => item.Key == status)?.Count ?? 0);
            }
            serverAdmins.Set(await db.Users.CountAsync(user => user.IsServerAdmin && user.Status == "active", cancel));
            var byKind = await db.Projects.AsNoTracking().GroupBy(project => project.Kind)
                .Select(group => new { group.Key, Count = group.Count() }).ToListAsync(cancel);
            foreach (var kind in new[] { "personal", "team" })
            {
                projects.WithLabels(kind).Set(byKind.FirstOrDefault(item => item.Key == kind)?.Count ?? 0);
            }
            memberships.Set(await db.Memberships.CountAsync(cancel));
            entities.WithLabels("collection").Set(await db.Collections.CountAsync(cancel));
            entities.WithLabels("folder").Set(await db.Folders.CountAsync(cancel));
            entities.WithLabels("request").Set(await db.Requests.CountAsync(cancel));
            entities.WithLabels("environment").Set(await db.Environments.CountAsync(cancel));
            variables.WithLabels("shared").Set(await db.Variables.CountAsync(variable => !variable.Secret, cancel));
            variables.WithLabels("secret").Set(await db.Variables.CountAsync(variable => variable.Secret, cancel));
            values.WithLabels("shared").Set(await db.SharedValues.CountAsync(cancel));
            values.WithLabels("vault").Set(await db.VaultValues.CountAsync(cancel));
            vaults.Set(await db.AccountVaults.CountAsync(cancel));
            snapshotSeconds.Set(Stopwatch.GetElapsedTime(started).TotalSeconds);
            Interlocked.Exchange(ref snapshotTaken, Stopwatch.GetTimestamp());
        }
        catch (Exception error) when (error is not OperationCanceledException)
        {
            // An unavailable database keeps the previous values; runtime metrics still export.
        }
        finally
        {
            refreshing.Release();
        }
    }

    public void Dispose()
    {
        meters.Dispose();
        refreshing.Dispose();
    }
}

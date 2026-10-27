using System.Data.Common;
using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text.Json;
using Fluxkit.Server;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Diagnostics;
using Microsoft.EntityFrameworkCore.Infrastructure;
using Microsoft.EntityFrameworkCore.Migrations;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.Extensions.Logging;
using Npgsql;
using Xunit;

namespace Fluxkit.Server.Tests;

// Real PostgreSQL is intentional: an in-memory provider cannot prove SQL compare-and-swap.
public sealed class ServerFixture : WebApplicationFactory<Program>, IAsyncLifetime
{
    private readonly string databaseName = "fluxkit_test_" + Guid.NewGuid().ToString("N");
    private readonly string adminConnection = Environment.GetEnvironmentVariable("FLUXKIT_TEST_DATABASE")
        ?? throw new InvalidOperationException("Set FLUXKIT_TEST_DATABASE to an isolated PostgreSQL test instance (database creation permission required).");
    public readonly ConcurrentReadGate Gate = new();
    private string Connection => new NpgsqlConnectionStringBuilder(adminConnection) { Database = databaseName }.ConnectionString;

    protected override void ConfigureWebHost(IWebHostBuilder builder)
    {
        builder.ConfigureLogging(logging => logging.ClearProviders());
        builder.ConfigureServices(services =>
        {
            services.AddDbContext<FluxkitDb>(options => options.UseNpgsql(Connection).AddInterceptors(Gate));
            services.AddSingleton<IStartupFilter, TestClientAddresses>();
        });
    }

    private int nextClient;
    protected override void ConfigureClient(HttpClient client)
    {
        base.ConfigureClient(client);
        client.DefaultRequestHeaders.Add("X-Test-Client", Interlocked.Increment(ref nextClient).ToString());
    }
    // TestServer has no socket peer. Give each synthetic client its own address so the
    // real authentication rate limiter keeps independent clients in separate partitions.
    private sealed class TestClientAddresses : IStartupFilter
    {
        public Action<IApplicationBuilder> Configure(Action<IApplicationBuilder> next) => app =>
        {
            app.Use(async (context, nextMiddleware) =>
            {
                if (int.TryParse(context.Request.Headers["X-Test-Client"], out var client))
                {
                    context.Connection.RemoteIpAddress = IPAddress.Parse($"127.0.0.{client % 250 + 1}");
                }
                await nextMiddleware();
            });
            next(app);
        };
    }

    async Task IAsyncLifetime.InitializeAsync()
    {
        await using var connection = new NpgsqlConnection(adminConnection);
        await connection.OpenAsync();
        await using var create = new NpgsqlCommand($"CREATE DATABASE {databaseName}", connection);
        await create.ExecuteNonQueryAsync();
        // These variables exist only inside this test process. No developer configuration is read.
        Environment.SetEnvironmentVariable("ConnectionStrings__Database", Connection);
        Environment.SetEnvironmentVariable("Auth__SigningKey", "Synthetic-test-signing-key-32-bytes-minimum");
        Environment.SetEnvironmentVariable("Bootstrap__AdminUsername", "testadmin");
        Environment.SetEnvironmentVariable("Bootstrap__AdminPassword", "SyntheticAdmin42!password");
        Environment.SetEnvironmentVariable("Auth__Mode", "local");
        using var client = CreateClient(); // Run production migrations and bootstrap.
        Assert.Equal(HttpStatusCode.OK, (await client.GetAsync("/health/ready")).StatusCode);
    }

    async Task IAsyncLifetime.DisposeAsync()
    {
        await base.DisposeAsync();
        NpgsqlConnection.ClearAllPools();
        await using var connection = new NpgsqlConnection(adminConnection);
        await connection.OpenAsync();
        await using var drop = new NpgsqlCommand($"DROP DATABASE {databaseName} WITH (FORCE)", connection);
        await drop.ExecuteNonQueryAsync();
    }

    public WebApplicationFactory<Program> Oidc(FakeOidcProvider provider)
    {
        var values = new Dictionary<string, string>
        {
            ["Auth__Mode"] = "oidc",
            ["Auth__Oidc__Authority"] = FakeOidcProvider.Issuer,
            ["Auth__Oidc__PublicOrigin"] = "https://localhost",
            ["Auth__Oidc__ClientId"] = "synthetic-client",
            ["Auth__Oidc__ClientSecret"] = "Synthetic-confidential-client-secret",
            ["Bootstrap__OidcSubject"] = provider.AdminSubject
        };
        var previous = values.ToDictionary(item => item.Key, item => Environment.GetEnvironmentVariable(item.Key));
        WebApplicationFactory<Program>? factory = null;
        try
        {
            foreach (var item in values) { Environment.SetEnvironmentVariable(item.Key, item.Value); }
            factory = WithWebHostBuilder(builder => builder.ConfigureServices(services =>
                services.PostConfigure<Microsoft.AspNetCore.Authentication.OpenIdConnect.OpenIdConnectOptions>("oidc",
                    options =>
                    {
                        options.Backchannel = new HttpClient(provider);
                        options.ConfigurationManager = new Microsoft.IdentityModel.Protocols.ConfigurationManager<Microsoft.IdentityModel.Protocols.OpenIdConnect.OpenIdConnectConfiguration>(
                            FakeOidcProvider.Issuer + "/.well-known/openid-configuration",
                            new Microsoft.IdentityModel.Protocols.OpenIdConnect.OpenIdConnectConfigurationRetriever(),
                            new Microsoft.IdentityModel.Protocols.HttpDocumentRetriever(options.Backchannel));
                    })));
            using var client = factory.CreateClient(); // Build while the exclusive-mode configuration is applied.
            return factory;
        }
        catch { factory?.Dispose(); throw; }
        finally { foreach (var item in previous) { Environment.SetEnvironmentVariable(item.Key, item.Value); } }
    }

    public async Task VerifyUpgrade()
    {
        var name = "fluxkit_upgrade_" + Guid.NewGuid().ToString("N");
        await using var connection = new NpgsqlConnection(adminConnection);
        await connection.OpenAsync();
        await using (var create = new NpgsqlCommand($"CREATE DATABASE {name}", connection)) { await create.ExecuteNonQueryAsync(); }
        try
        {
            var options = new DbContextOptionsBuilder<FluxkitDb>().UseNpgsql(new NpgsqlConnectionStringBuilder(adminConnection) { Database = name }.ConnectionString).Options;
            await using var db = new FluxkitDb(options);
            await db.GetService<IMigrator>().MigrateAsync("20261026072628_InitialSchema");
            var id = Guid.NewGuid(); var owner = Guid.NewGuid(); var snapshot = "{\"project\":{\"name\":\"Existing project\"}}";
            await db.Database.ExecuteSqlInterpolatedAsync($"INSERT INTO \"Projects\" (\"Id\", \"OwnerId\", \"Name\", \"Kind\", \"Snapshot\", \"Revision\", \"UpdatedAt\") VALUES ({id}, {owner}, 'Existing project', 'personal', CAST({snapshot} AS jsonb), 7, {DateTimeOffset.UtcNow})");
            await db.Database.MigrateAsync();
            var project = await db.Projects.AsNoTracking().SingleAsync(project => project.Id == id);
            Assert.Equal("Existing project", project.Name); Assert.Equal(7, project.Revision);
            Assert.Equal("[]", project.SharedValues); Assert.Equal(0, project.SharedValuesRevision);
            Assert.Empty(await db.AccountVaults.ToListAsync()); Assert.Empty(await db.ProjectSecrets.ToListAsync());
        }
        finally
        {
            NpgsqlConnection.ClearAllPools();
            await using var drop = new NpgsqlCommand($"DROP DATABASE {name} WITH (FORCE)", connection); await drop.ExecuteNonQueryAsync();
        }
    }

    public async Task<HttpClient> Login(string username, string password)
    {
        var client = CreateClient();
        var response = await client.PostAsJsonAsync("/api/auth/login", new { username, password });
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var token = (await response.Content.ReadFromJsonAsync<JsonElement>()).GetProperty("accessToken").GetString();
        client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", token);
        return client;
    }
    public Task<HttpClient> Admin() => Login("testadmin", "SyntheticAdmin42!password");
    public async Task<(Guid Id, string Username)> Register()
    {
        var username = "user" + Guid.NewGuid().ToString("N");
        using var client = CreateClient();
        Assert.Equal(HttpStatusCode.Accepted, (await client.PostAsJsonAsync("/api/auth/register", new { username, password = "SyntheticUser42!password" })).StatusCode);
        using var scope = Services.CreateScope();
        var user = await scope.ServiceProvider.GetRequiredService<FluxkitDb>().Users.SingleAsync(user => user.UserName == username);
        return (user.Id, username);
    }
    public async Task<HttpClient> ApprovedUser()
    {
        var user = await Register();
        using var admin = await Admin();
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PostAsync($"/api/admin/users/{user.Id}/approve", null)).StatusCode);
        return await Login(user.Username, "SyntheticUser42!password");
    }
    public static async Task<JsonElement> PersonalProject(HttpClient client)
    {
        var projects = await client.GetFromJsonAsync<JsonElement>("/api/projects");
        var id = projects.EnumerateArray().Single(project => project.GetProperty("kind").GetString() == "personal").GetProperty("id").GetGuid();
        return await client.GetFromJsonAsync<JsonElement>($"/api/projects/{id}");
    }
}

/// Hold both snapshot reads before either update, reproducing the former lost-update race.
public sealed class ConcurrentReadGate : DbCommandInterceptor
{
    private Guid projectId;
    private int reads;
    private TaskCompletionSource? ready;
    public void Arm(Guid id) { projectId = id; reads = 0; ready = new(TaskCreationOptions.RunContinuationsAsynchronously); }
    public override async ValueTask<DbDataReader> ReaderExecutedAsync(DbCommand command, CommandExecutedEventData eventData, DbDataReader result, CancellationToken cancellationToken = default)
    {
        var barrier = ready;
        if (barrier is not null && command.CommandText.Contains("FROM \"Projects\"")
            && command.Parameters.Cast<DbParameter>().Any(parameter => parameter.Value is Guid id && id == projectId))
        {
            if (Interlocked.Increment(ref reads) >= 2)
            {
                barrier.TrySetResult();
            }
            await barrier.Task.WaitAsync(TimeSpan.FromSeconds(10), cancellationToken);
        }
        return result;
    }
}

public sealed partial class ServerTests(ServerFixture server) : IClassFixture<ServerFixture>
{
    private static HttpRequestMessage Update(Guid id, long? revision, string snapshot)
    {
        var request = new HttpRequestMessage(HttpMethod.Put, $"/api/projects/{id}") { Content = JsonContent.Create(new { snapshot }) };
        if (revision is not null)
        {
            request.Headers.TryAddWithoutValidation("If-Match", $"\"{revision}\"");
        }
        return request;
    }
    private static string Snapshot(Guid id, string name) => JsonSerializer.Serialize(new
    {
        project = new { schemaVersion = 1, id, name },
        collections = Array.Empty<object>(),
        environments = new { schemaVersion = 1, environments = Array.Empty<object>(), variables = Array.Empty<object>() }
    });

    [Fact]
    public async Task Concurrent_updates_accept_exactly_one_writer_and_preserve_the_winner()
    {
        using var owner = await server.ApprovedUser();
        var project = await ServerFixture.PersonalProject(owner);
        var id = project.GetProperty("id").GetGuid();
        var revision = project.GetProperty("revision").GetInt64();
        server.Gate.Arm(id);
        var results = await Task.WhenAll(owner.SendAsync(Update(id, revision, Snapshot(id, "First writer"))), owner.SendAsync(Update(id, revision, Snapshot(id, "Second writer"))));
        Assert.Single(results, response => response.StatusCode == HttpStatusCode.OK);
        Assert.Single(results, response => response.StatusCode == HttpStatusCode.Conflict);
        var winner = await results.Single(response => response.IsSuccessStatusCode).Content.ReadFromJsonAsync<JsonElement>();
        var saved = await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}");
        Assert.Equal(revision + 1, saved.GetProperty("revision").GetInt64());
        Assert.Equal(winner.GetProperty("name").GetString(), saved.GetProperty("name").GetString());
        Assert.Equal($"\"{revision + 1}\"", results.Single(response => response.IsSuccessStatusCode).Headers.ETag?.Tag);
    }

    [Fact]
    public async Task Missing_or_stale_preconditions_preserve_the_saved_snapshot()
    {
        using var owner = await server.ApprovedUser();
        var before = await ServerFixture.PersonalProject(owner);
        var id = before.GetProperty("id").GetGuid();
        Assert.Equal((HttpStatusCode)428, (await owner.SendAsync(Update(id, null, Snapshot(id, "Missing")))).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await owner.SendAsync(Update(id, 0, Snapshot(id, "Stale")))).StatusCode);
        var after = await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}");
        Assert.Equal(before.GetProperty("snapshot").GetString(), after.GetProperty("snapshot").GetString());
        Assert.Equal(before.GetProperty("revision").GetInt64(), after.GetProperty("revision").GetInt64());
    }

    [Fact]
    public async Task Other_accounts_and_server_admin_cannot_read_or_replace_personal_projects()
    {
        using var owner = await server.ApprovedUser();
        using var stranger = await server.ApprovedUser();
        using var admin = await server.Admin();
        var project = await ServerFixture.PersonalProject(owner);
        var id = project.GetProperty("id").GetGuid();
        foreach (var client in new[] { stranger, admin })
        {
            Assert.Equal(HttpStatusCode.NotFound, (await client.GetAsync($"/api/projects/{id}")).StatusCode);
            Assert.Equal(HttpStatusCode.NotFound, (await client.SendAsync(Update(id, 1, Snapshot(id, "Unauthorized")))).StatusCode);
            Assert.DoesNotContain((await client.GetFromJsonAsync<JsonElement>("/api/projects")).EnumerateArray(), row => row.GetProperty("id").GetGuid() == id);
        }
    }

    [Fact]
    public async Task Pending_accounts_cannot_sign_in_and_suspension_invalidates_an_existing_token()
    {
        var pending = await server.Register();
        using var anonymous = server.CreateClient();
        Assert.Equal(HttpStatusCode.Unauthorized, (await anonymous.PostAsJsonAsync("/api/auth/login", new { username = pending.Username, password = "SyntheticUser42!password" })).StatusCode);
        using var active = await server.ApprovedUser();
        var account = await active.GetFromJsonAsync<JsonElement>("/api/account");
        using var admin = await server.Admin();
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PutAsJsonAsync($"/api/admin/users/{account.GetProperty("id").GetGuid()}/status", new { status = "disabled" })).StatusCode);
        Assert.Equal(HttpStatusCode.Unauthorized, (await active.GetAsync("/api/projects")).StatusCode);
    }

    [Fact]
    public async Task Secret_envelopes_are_scoped_to_the_caller_even_in_a_shared_project()
    {
        using var admin = await server.Admin();
        using var member = await server.ApprovedUser();
        var account = await member.GetFromJsonAsync<JsonElement>("/api/account");
        var created = await admin.PostAsJsonAsync("/api/projects", new { name = "Shared synthetic project" });
        var project = await created.Content.ReadFromJsonAsync<JsonElement>();
        var id = project.GetProperty("id").GetGuid();
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PostAsJsonAsync($"/api/projects/{id}/members", new { username = account.GetProperty("userName").GetString(), role = "member" })).StatusCode);
        var variableId = Guid.NewGuid();
        var envelope = new { variableId, environmentId = "default", salt = Convert.ToBase64String(new byte[16]), nonce = Convert.ToBase64String(new byte[12]), ciphertext = Convert.ToBase64String(new byte[16]), wrappedKey = Convert.ToBase64String(new byte[48]), wrappedKeyNonce = Convert.ToBase64String(new byte[12]), algorithm = "AES-256-GCM", kdfParameters = "{\"algorithm\":\"Argon2id\",\"version\":19,\"memoryKiB\":65536,\"iterations\":3,\"parallelism\":1}" };
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PutAsJsonAsync($"/api/projects/{id}/secrets", envelope)).StatusCode);
        Assert.Empty((await member.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/secrets")).EnumerateArray());
        Assert.Equal(HttpStatusCode.NoContent, (await member.DeleteAsync($"/api/projects/{id}/secrets/{variableId}/default")).StatusCode);
        Assert.Single((await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/secrets")).EnumerateArray());
        var changed = await member.SendAsync(Update(id, 1, Snapshot(id, "Member can edit")));
        Assert.Equal(HttpStatusCode.OK, changed.StatusCode);
        Assert.Equal(HttpStatusCode.NoContent, (await admin.DeleteAsync($"/api/projects/{id}/members/{account.GetProperty("id").GetGuid()}")).StatusCode);
        Assert.Equal(HttpStatusCode.NotFound, (await member.GetAsync($"/api/projects/{id}")).StatusCode);
    }
    [Fact]
    public Task Value_schema_upgrade_preserves_existing_project_snapshots_and_revisions() => server.VerifyUpgrade();

    private static async Task<(Guid Id, Guid Ordinary, Guid Secret, Guid Environment)> ValueProject(HttpClient owner)
    {
        var existing = await ServerFixture.PersonalProject(owner);
        var id = existing.GetProperty("id").GetGuid();
        var ordinary = Guid.NewGuid(); var secret = Guid.NewGuid(); var environment = Guid.NewGuid();
        var snapshot = ValueSnapshot(id, ordinary, secret, environment);
        Assert.Equal(HttpStatusCode.OK, (await owner.SendAsync(Update(id, 1, snapshot))).StatusCode);
        return (id, ordinary, secret, environment);
    }
    private static string ValueSnapshot(Guid id, Guid ordinary, Guid secret, Guid environment) => JsonSerializer.Serialize(new
    {
        project = new { schemaVersion = 1, id, name = "Value test project" },
        collections = Array.Empty<object>(),
        environments = new
        {
            schemaVersion = 1,
            environments = new[] { new { id = environment, name = "Test" } },
            variables = new object[] { new { id = ordinary, name = "baseUrl", secret = false }, new { id = secret, name = "apiToken" } }
        }
    });
    private static VaultProfile Profile(byte marker = 0) => new(1, "AES-256-GCM", new("Argon2id", 19, 65536, 3, 1),
        Convert.ToBase64String(new byte[16]), Convert.ToBase64String(new byte[12]), Convert.ToBase64String(Enumerable.Repeat(marker, 48).ToArray()));
    private static Task<HttpResponseMessage> PutValues<T>(HttpClient client, string path, long revision, T body)
    {
        var request = new HttpRequestMessage(HttpMethod.Put, path) { Content = JsonContent.Create(body) };
        request.Headers.TryAddWithoutValidation("If-Match", $"\"{revision}\"");
        return client.SendAsync(request);
    }
    private static CipherValue Cipher(Guid variable, string environment = "default", byte marker = 0) => new(variable, environment,
        Convert.ToBase64String(new byte[12]), Convert.ToBase64String(Enumerable.Repeat(marker, 16).ToArray()));

    [Fact]
    public async Task Shared_values_are_visible_to_members_but_never_promote_legacy_secrets()
    {
        using var owner = await server.Admin(); using var member = await server.ApprovedUser();
        var account = await member.GetFromJsonAsync<JsonElement>("/api/account");
        var create = await owner.PostAsJsonAsync("/api/projects", new { name = "Shared values" });
        var id = (await create.Content.ReadFromJsonAsync<JsonElement>()).GetProperty("id").GetGuid();
        var ordinary = Guid.NewGuid(); var secret = Guid.NewGuid(); var environment = Guid.NewGuid();
        Assert.Equal(HttpStatusCode.OK, (await owner.SendAsync(Update(id, 1, ValueSnapshot(id, ordinary, secret, environment)))).StatusCode);
        Assert.Equal(HttpStatusCode.NoContent, (await owner.PostAsJsonAsync($"/api/projects/{id}/members", new { username = account.GetProperty("userName").GetString(), role = "member" })).StatusCode);
        var values = new[] { new SharedValue(ordinary, "default", "https://synthetic.example.test"), new SharedValue(ordinary, environment.ToString(), "") };
        Assert.Equal(HttpStatusCode.OK, (await PutValues(member, $"/api/projects/{id}/values", 0, new { projectRevision = 2, values })).StatusCode);
        var saved = await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/values");
        Assert.Equal(2, saved.GetProperty("values").GetArrayLength());
        Assert.Equal("", saved.GetProperty("values")[1].GetProperty("value").GetString());
        var forbidden = new[] { new SharedValue(secret, "default", "Do not publish a secret") };
        Assert.Equal(HttpStatusCode.BadRequest, (await PutValues(member, $"/api/projects/{id}/values", 1, new { projectRevision = 2, values = forbidden })).StatusCode);
        Assert.DoesNotContain("synthetic.example", (await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}")).GetProperty("snapshot").GetString()!);
        foreach (var client in new[] { owner, member })
        {
            Assert.Equal(HttpStatusCode.OK, (await PutValues(client, "/api/account/vault", 0, new { profile = Profile() })).StatusCode);
        }
        Assert.Equal(HttpStatusCode.OK, (await PutValues(owner, $"/api/projects/{id}/vault-values", 0, new { projectRevision = 2, values = new[] { Cipher(secret, marker: 1) } })).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await PutValues(member, $"/api/projects/{id}/vault-values", 0, new { projectRevision = 2, values = new[] { Cipher(secret, marker: 2) } })).StatusCode);
        var ownerCipher = (await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/vault-values")).GetProperty("values")[0].GetProperty("ciphertext").GetString();
        var memberCipher = (await member.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/vault-values")).GetProperty("values")[0].GetProperty("ciphertext").GetString();
        Assert.NotEqual(ownerCipher, memberCipher);
        Assert.Equal(HttpStatusCode.OK, (await PutValues(member, $"/api/projects/{id}/vault-values", 1, new { projectRevision = 2, values = Array.Empty<CipherValue>() })).StatusCode);
        Assert.Single((await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/vault-values")).GetProperty("values").EnumerateArray());
    }

    [Fact]
    public async Task Shared_value_updates_have_atomic_revision_and_definition_preconditions()
    {
        using var owner = await server.ApprovedUser();
        var project = await ValueProject(owner);
        server.Gate.Arm(project.Id);
        var requests = new[] { "First", "Second" }.Select(value => PutValues(owner, $"/api/projects/{project.Id}/values", 0,
            new { projectRevision = 2, values = new[] { new SharedValue(project.Ordinary, "default", value) } }));
        var responses = await Task.WhenAll(requests);
        Assert.Single(responses, response => response.StatusCode == HttpStatusCode.OK);
        Assert.Single(responses, response => response.StatusCode == HttpStatusCode.Conflict);
        Assert.Equal(HttpStatusCode.Conflict, (await PutValues(owner, $"/api/projects/{project.Id}/values", 1,
            new { projectRevision = 1, values = Array.Empty<SharedValue>() })).StatusCode);
        var saved = await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{project.Id}/values");
        Assert.Equal(1, saved.GetProperty("revision").GetInt64());
        Assert.Single(saved.GetProperty("values").EnumerateArray());
    }

    [Fact]
    public async Task Vault_profiles_are_private_revision_checked_and_reject_unsafe_kdf_costs()
    {
        using var first = await server.ApprovedUser(); using var other = await server.ApprovedUser();
        Assert.Equal(HttpStatusCode.OK, (await PutValues(first, "/api/account/vault", 0, new { profile = Profile() })).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await PutValues(first, "/api/account/vault", 0, new { profile = Profile(1) })).StatusCode);
        Assert.Equal(JsonValueKind.Null, (await other.GetFromJsonAsync<JsonElement>("/api/account/vault")).GetProperty("profile").ValueKind);
        var unsafeProfile = Profile() with { Kdf = new("Argon2id", 19, int.MaxValue, 3, 1) };
        Assert.Equal(HttpStatusCode.BadRequest, (await PutValues(first, "/api/account/vault", 1, new { profile = unsafeProfile })).StatusCode);
        Assert.Equal(1, (await first.GetFromJsonAsync<JsonElement>("/api/account/vault")).GetProperty("revision").GetInt64());
    }

    [Fact]
    public async Task Encrypted_value_bundles_are_user_scoped_and_profile_rewrap_preserves_ciphertext()
    {
        using var owner = await server.ApprovedUser();
        var project = await ValueProject(owner);
        var cipher = Cipher(project.Secret);
        Assert.Equal(HttpStatusCode.BadRequest, (await PutValues(owner, $"/api/projects/{project.Id}/vault-values", 0,
            new { projectRevision = 2, values = new[] { cipher } })).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await PutValues(owner, "/api/account/vault", 0, new { profile = Profile() })).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await PutValues(owner, $"/api/projects/{project.Id}/vault-values", 0,
            new { projectRevision = 2, values = new[] { cipher } })).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await PutValues(owner, $"/api/projects/{project.Id}/vault-values", 0,
            new { projectRevision = 2, values = Array.Empty<CipherValue>() })).StatusCode);
        var before = await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{project.Id}/vault-values");
        Assert.Equal(HttpStatusCode.OK, (await PutValues(owner, "/api/account/vault", 1, new { profile = Profile(1) })).StatusCode);
        var after = await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{project.Id}/vault-values");
        Assert.Equal(before.GetProperty("values").ToString(), after.GetProperty("values").ToString());
        Assert.Equal(HttpStatusCode.BadRequest, (await PutValues(owner, $"/api/projects/{project.Id}/vault-values", 1,
            new { projectRevision = 2, values = new[] { Cipher(project.Ordinary) } })).StatusCode);
    }

    [Fact]
    public async Task Transfer_commits_selected_shared_and_cipher_values_together_and_failure_creates_no_project()
    {
        using var owner = await server.ApprovedUser();
        var id = Guid.NewGuid(); var ordinary = Guid.NewGuid(); var secret = Guid.NewGuid(); var environment = Guid.NewGuid();
        var snapshot = ValueSnapshot(id, ordinary, secret, environment);
        var path = $"/api/projects/{id}/takeover";
        Assert.Equal(HttpStatusCode.BadRequest, (await owner.PostAsJsonAsync(path, new
        {
            name = "Value test project",
            kind = "personal",
            snapshot,
            encryptedSecrets = Array.Empty<object>(),
            sharedValues = new[] { new SharedValue(secret, "default", "Must stay secret") }
        })).StatusCode);
        Assert.Equal(HttpStatusCode.NotFound, (await owner.GetAsync($"/api/projects/{id}")).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await PutValues(owner, "/api/account/vault", 0, new { profile = Profile() })).StatusCode);
        Assert.Equal(HttpStatusCode.Created, (await owner.PostAsJsonAsync(path, new
        {
            name = "Value test project",
            kind = "personal",
            snapshot,
            encryptedSecrets = Array.Empty<object>(),
            sharedValues = new[] { new SharedValue(ordinary, "default", "Shared synthetic") },
            vaultValues = new[] { Cipher(secret) }
        })).StatusCode);
        Assert.Single((await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/values")).GetProperty("values").EnumerateArray());
        Assert.Single((await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/vault-values")).GetProperty("values").EnumerateArray());
        Assert.Equal(HttpStatusCode.Conflict, (await owner.PostAsJsonAsync(path, new { name = "Value test project", kind = "personal", snapshot, encryptedSecrets = Array.Empty<object>() })).StatusCode);
    }

    [Fact]
    public async Task Values_enforce_slot_limits_and_unauthorized_users_cannot_read_or_write_them()
    {
        using var owner = await server.ApprovedUser(); using var stranger = await server.ApprovedUser();
        var project = await ValueProject(owner);
        foreach (var route in new[] { "values", "vault-values" })
        {
            Assert.Equal(HttpStatusCode.NotFound, (await stranger.GetAsync($"/api/projects/{project.Id}/{route}")).StatusCode);
            Assert.Equal(HttpStatusCode.NotFound, (await PutValues(stranger, $"/api/projects/{project.Id}/{route}", 0,
                new { projectRevision = 2, values = Array.Empty<object>() })).StatusCode);
        }
        foreach (var values in new[] {
            new[] { new SharedValue(project.Ordinary, "default", new string('x', 2561)) },
            new[] { new SharedValue(project.Ordinary, Guid.NewGuid().ToString(), "Unknown environment") },
            new[] { new SharedValue(project.Ordinary, "default", "A"), new SharedValue(project.Ordinary, "default", "B") } })
        {
            Assert.Equal(HttpStatusCode.BadRequest, (await PutValues(owner, $"/api/projects/{project.Id}/values", 0, new { projectRevision = 2, values })).StatusCode);
        }
    }

}

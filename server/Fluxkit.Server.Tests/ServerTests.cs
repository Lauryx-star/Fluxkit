using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text.Json;
using Fluxkit.Server;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.EntityFrameworkCore;
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
    private string Connection => new NpgsqlConnectionStringBuilder(adminConnection) { Database = databaseName }.ConnectionString;

    protected override void ConfigureWebHost(IWebHostBuilder builder)
    {
        builder.ConfigureLogging(logging => logging.ClearProviders());
        builder.ConfigureServices(services =>
        {
            services.AddDbContext<FluxkitDb>(options => options.UseNpgsql(Connection));
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

public sealed partial class ServerTests(ServerFixture server) : IClassFixture<ServerFixture>
{
    [Fact]
    public async Task Other_accounts_and_server_admin_cannot_read_personal_projects()
    {
        using var owner = await server.ApprovedUser();
        using var stranger = await server.ApprovedUser();
        using var admin = await server.Admin();
        var project = await ServerFixture.PersonalProject(owner);
        var id = project.GetProperty("id").GetGuid();
        foreach (var client in new[] { stranger, admin })
        {
            Assert.Equal(HttpStatusCode.NotFound, (await client.GetAsync($"/api/projects/{id}")).StatusCode);
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
    public async Task Team_members_can_read_until_they_are_removed()
    {
        using var admin = await server.Admin();
        using var member = await server.ApprovedUser();
        var account = await member.GetFromJsonAsync<JsonElement>("/api/account");
        var created = await admin.PostAsJsonAsync("/api/projects", new { name = "Shared synthetic project" });
        var id = (await created.Content.ReadFromJsonAsync<JsonElement>()).GetProperty("id").GetGuid();
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PostAsJsonAsync($"/api/projects/{id}/members", new { username = account.GetProperty("userName").GetString(), role = "member" })).StatusCode);
        Assert.True((await member.GetFromJsonAsync<JsonElement>($"/api/projects/{id}")).GetProperty("canEdit").GetBoolean());
        Assert.Equal(HttpStatusCode.NoContent, (await admin.DeleteAsync($"/api/projects/{id}/members/{account.GetProperty("id").GetGuid()}")).StatusCode);
        Assert.Equal(HttpStatusCode.NotFound, (await member.GetAsync($"/api/projects/{id}")).StatusCode);
    }

    private static VaultProfile Profile(byte marker = 0) => new(1, "AES-256-GCM", new("Argon2id", 19, 65536, 3, 1),
        Convert.ToBase64String(new byte[16]), Convert.ToBase64String(new byte[12]), Convert.ToBase64String(Enumerable.Repeat(marker, 48).ToArray()));
    private static Task<HttpResponseMessage> PutWithRevision<T>(HttpClient client, string path, long revision, T body)
    {
        var request = new HttpRequestMessage(HttpMethod.Put, path) { Content = JsonContent.Create(body) };
        request.Headers.TryAddWithoutValidation("If-Match", $"\"{revision}\"");
        return client.SendAsync(request);
    }

    [Fact]
    public async Task Vault_profiles_are_private_revision_checked_and_reject_unsafe_kdf_costs()
    {
        using var first = await server.ApprovedUser(); using var other = await server.ApprovedUser();
        Assert.Equal(HttpStatusCode.OK, (await PutWithRevision(first, "/api/account/vault", 0, new { profile = Profile() })).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await PutWithRevision(first, "/api/account/vault", 0, new { profile = Profile(1) })).StatusCode);
        Assert.Equal(JsonValueKind.Null, (await other.GetFromJsonAsync<JsonElement>("/api/account/vault")).GetProperty("profile").ValueKind);
        var unsafeProfile = Profile() with { Kdf = new("Argon2id", 19, int.MaxValue, 3, 1) };
        Assert.Equal(HttpStatusCode.BadRequest, (await PutWithRevision(first, "/api/account/vault", 1, new { profile = unsafeProfile })).StatusCode);
        Assert.Equal(1, (await first.GetFromJsonAsync<JsonElement>("/api/account/vault")).GetProperty("revision").GetInt64());
    }
}

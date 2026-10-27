using System.IdentityModel.Tokens.Jwt;
using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Security.Claims;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using Fluxkit.Server;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.AspNetCore.WebUtilities;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Microsoft.IdentityModel.Tokens;
using Xunit;

namespace Fluxkit.Server.Tests;

public sealed class FakeOidcProvider : HttpMessageHandler
{
    public const string Issuer = "https://identity.synthetic.test";
    public string AdminSubject { get; } = "admin-" + Guid.NewGuid();
    private readonly RSA rsa = RSA.Create(2048);
    private readonly Dictionary<string, (string Nonce, string Challenge, string Subject, string Fault)> codes = [];
    public bool SawPkceAndSecret { get; private set; }
    public string Code(Uri authorization, string subject, string fault = "")
    {
        var query = QueryHelpers.ParseQuery(authorization.Query);
        Assert.Equal("code", query["response_type"]);
        Assert.Equal("S256", query["code_challenge_method"]);
        Assert.Equal("https://localhost/api/auth/oidc/callback", query["redirect_uri"]);
        var code = Guid.NewGuid().ToString();
        codes.Add(code, (query["nonce"].ToString(), query["code_challenge"].ToString(), subject, fault));
        return code;
    }
    protected override async Task<HttpResponseMessage> SendAsync(HttpRequestMessage request, CancellationToken ct)
    {
        object body;
        if (request.RequestUri!.AbsolutePath.EndsWith("openid-configuration"))
        {
            body = new
            {
                issuer = Issuer,
                authorization_endpoint = Issuer + "/authorize",
                token_endpoint = Issuer + "/token",
                jwks_uri = Issuer + "/keys",
                response_types_supported = new[] { "code" },
                subject_types_supported = new[] { "public" },
                id_token_signing_alg_values_supported = new[] { "RS256" },
                code_challenge_methods_supported = new[] { "S256" }
            };
        }
        else if (request.RequestUri.AbsolutePath == "/keys")
        {
            var key = rsa.ExportParameters(false);
            body = new
            {
                keys = new[] { new { kty = "RSA", kid = "synthetic-key", use = "sig", alg = "RS256",
                n = Base64UrlEncoder.Encode(key.Modulus!), e = Base64UrlEncoder.Encode(key.Exponent!) } }
            };
        }
        else
        {
            Assert.Equal("/token", request.RequestUri.AbsolutePath);
            var form = QueryHelpers.ParseQuery(await request.Content!.ReadAsStringAsync(ct));
            Assert.True(codes.Remove(form["code"].ToString(), out var code)); // Authorization codes cannot be replayed.
            Assert.Equal(code.Challenge, Base64UrlEncoder.Encode(SHA256.HashData(Encoding.ASCII.GetBytes(form["code_verifier"].ToString()))));
            Assert.Equal("Synthetic-confidential-client-secret", form["client_secret"]);
            Assert.Equal("https://localhost/api/auth/oidc/callback", form["redirect_uri"]);
            SawPkceAndSecret = true;
            using var wrongKey = RSA.Create(2048);
            var key = new RsaSecurityKey(code.Fault == "signature" ? wrongKey : rsa) { KeyId = "synthetic-key" };
            var token = new JwtSecurityToken(code.Fault == "issuer" ? "https://wrong.synthetic.test" : Issuer,
                code.Fault == "audience" ? "another-client" : "synthetic-client",
                new[] { new Claim("iat", DateTimeOffset.UtcNow.ToUnixTimeSeconds().ToString(), ClaimValueTypes.Integer64), new Claim("sub", code.Subject), new Claim("nonce", code.Fault == "nonce" ? "wrong-nonce" : code.Nonce),
                    new Claim("preferred_username", "same-display-name"), new Claim("email", "same@synthetic.test") },
                DateTime.UtcNow.AddMinutes(-1), code.Fault == "expired" ? DateTime.UtcNow.AddSeconds(-40) : DateTime.UtcNow.AddMinutes(5),
                new SigningCredentials(key, SecurityAlgorithms.RsaSha256));
            body = new
            {
                access_token = "synthetic-provider-token-never-stored",
                token_type = "Bearer",
                expires_in = 300,
                id_token = new JwtSecurityTokenHandler().WriteToken(token)
            };
        }
        return new(HttpStatusCode.OK) { Content = JsonContent.Create(body) };
    }
    protected override void Dispose(bool disposing) { if (disposing) { rsa.Dispose(); } base.Dispose(disposing); }
}

public sealed partial class ServerTests
{
    private static HttpClient Browser(WebApplicationFactory<Program> factory) => factory.CreateClient(new()
    {
        BaseAddress = new Uri("https://localhost"),
        AllowAutoRedirect = false,
        HandleCookies = false
    });
    private static async Task<(JsonElement Start, Uri Redirect, string Cookies)> StartOidc(HttpClient client)
    {
        var start = await (await client.PostAsync("/api/auth/oidc/start", null)).Content.ReadFromJsonAsync<JsonElement>();
        var challenge = await client.GetAsync("/api/auth/oidc/authorize/" + start.GetProperty("id").GetString());
        Assert.Equal(HttpStatusCode.Redirect, challenge.StatusCode);
        return (start, challenge.Headers.Location!, string.Join("; ", challenge.Headers.GetValues("Set-Cookie").Select(value => value.Split(';')[0])));
    }
    private static async Task Callback(HttpClient client, FakeOidcProvider provider,
        (JsonElement Start, Uri Redirect, string Cookies) flow, string subject, string fault = "", bool cookie = true)
    {
        var state = QueryHelpers.ParseQuery(flow.Redirect.Query)["state"].ToString();
        using var request = new HttpRequestMessage(HttpMethod.Post, "/api/auth/oidc/callback")
        {
            Content = new FormUrlEncodedContent(new Dictionary<string, string>
            {
                ["state"] = fault == "state" ? "invalid-state" : state,
                ["code"] = provider.Code(flow.Redirect, subject, fault)
            })
        };
        if (cookie) { request.Headers.Add("Cookie", flow.Cookies); }
        var response = await client.SendAsync(request);
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.DoesNotContain("accessToken", await response.Content.ReadAsStringAsync());
        Assert.False(response.Headers.TryGetValues("Set-Cookie", out var cookies) && cookies.Any(value => value.StartsWith(".AspNetCore.oidc-unused")));
    }
    private static async Task<JsonElement> PollOidc(HttpClient client, JsonElement start, string? secret = null) =>
        await (await client.PostAsJsonAsync("/api/auth/oidc/poll", new
        {
            id = start.GetProperty("id").GetString(),
            secret = secret ?? start.GetProperty("secret").GetString()
        })).Content.ReadFromJsonAsync<JsonElement>();
    private static void Session(HttpClient client, JsonElement result)
    {
        Assert.Equal("complete", result.GetProperty("status").GetString());
        client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", result.GetProperty("session").GetProperty("accessToken").GetString());
    }

    [Fact]
    public async Task Oidc_mode_uses_real_code_pkce_validation_and_requires_approval_without_email_linking()
    {
        using var provider = new FakeOidcProvider(); using var factory = server.Oidc(provider);
        using var admin = Browser(factory);
        Assert.Equal("oidc", (await admin.GetFromJsonAsync<JsonElement>("/api/auth/mode")).GetProperty("mode").GetString());
        Assert.Equal(HttpStatusCode.NotFound, (await admin.PostAsJsonAsync("/api/auth/login", new { username = "testadmin", password = "SyntheticAdmin42!password" })).StatusCode);
        Assert.Equal(HttpStatusCode.NotFound, (await admin.PostAsJsonAsync("/api/auth/register", new { username = "nobody", password = "SyntheticAdmin42!password" })).StatusCode);
        var flow = await StartOidc(admin); await Callback(admin, provider, flow, provider.AdminSubject);
        Assert.Equal("expired", (await PollOidc(admin, flow.Start, Convert.ToBase64String(new byte[32]))).GetProperty("status").GetString());
        var result = await PollOidc(admin, flow.Start); Session(admin, result);
        var account = await admin.GetFromJsonAsync<JsonElement>("/api/account");
        Assert.True(account.GetProperty("isServerAdmin").GetBoolean());
        Assert.Single((await admin.GetFromJsonAsync<JsonElement>("/api/projects")).EnumerateArray());
        Assert.Equal("expired", (await PollOidc(admin, flow.Start)).GetProperty("status").GetString());
        using var member = Browser(factory);
        var subject = "member-" + Guid.NewGuid();
        flow = await StartOidc(member); await Callback(member, provider, flow, subject);
        Assert.Equal("pendingApproval", (await PollOidc(member, flow.Start)).GetProperty("status").GetString());
        Assert.Equal(HttpStatusCode.Unauthorized, (await member.GetAsync("/api/projects")).StatusCode);
        var pending = await admin.GetFromJsonAsync<JsonElement>("/api/admin/pending-users");
        // Same preferred username/email as admin never links to the administrator account.
        var entry = pending.EnumerateArray().Single(item => item.GetProperty("userName").GetString()!.StartsWith("same-display-name"));
        var id = entry.GetProperty("id").GetGuid(); Assert.NotEqual(account.GetProperty("id").GetGuid(), id);
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PostAsync($"/api/admin/users/{id}/approve", null)).StatusCode);
        using var returning = Browser(factory);
        flow = await StartOidc(returning); await Callback(returning, provider, flow, subject);
        Session(returning, await PollOidc(returning, flow.Start));
        Assert.Equal(id, (await returning.GetFromJsonAsync<JsonElement>("/api/account")).GetProperty("id").GetGuid());
        Assert.Single((await returning.GetFromJsonAsync<JsonElement>("/api/projects")).EnumerateArray());
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PutAsJsonAsync($"/api/admin/users/{id}/status", new { status = "disabled" })).StatusCode);
        Assert.Equal(HttpStatusCode.Unauthorized, (await returning.GetAsync("/api/projects")).StatusCode);
        Assert.True(provider.SawPkceAndSecret);
    }

    [Fact]
    public async Task Oidc_rejects_invalid_tokens_nonce_state_correlation_and_unapproved_mode_tokens()
    {
        using var provider = new FakeOidcProvider(); using var factory = server.Oidc(provider);
        foreach (var fault in new[] { "signature", "issuer", "audience", "nonce", "expired", "state", "cookie" })
        {
            using var browser = Browser(factory);
            var flow = await StartOidc(browser);
            await Callback(browser, provider, flow, "invalid-" + Guid.NewGuid(), fault, fault != "cookie");
            var result = await PollOidc(browser, flow.Start);
            Assert.DoesNotContain("session", result.ToString());
            Assert.Contains(result.GetProperty("status").GetString(), new[] { "failed", "waiting" });
        }
        using var local = await server.Admin();
        using var oidc = Browser(factory); oidc.DefaultRequestHeaders.Authorization = local.DefaultRequestHeaders.Authorization;
        Assert.Equal(HttpStatusCode.Unauthorized, (await oidc.GetAsync("/api/projects")).StatusCode);
        Assert.Equal("local", (await local.GetFromJsonAsync<JsonElement>("/api/auth/mode")).GetProperty("mode").GetString());
        Assert.Equal(HttpStatusCode.NotFound, (await local.PostAsync("/api/auth/oidc/start", null)).StatusCode);
    }

    [Fact]
    public void Oidc_handoffs_are_bounded_expire_and_are_single_use()
    {
        var clock = new TestClock(); var tickets = new OidcTickets(clock);
        var first = tickets.Start()!;
        Assert.True(tickets.Claim(first.Id)); Assert.False(tickets.Claim(first.Id));
        Assert.Equal("waiting", tickets.Poll(first.Id, first.Secret).Status);
        var user = Guid.NewGuid(); tickets.Complete(first.Id, user);
        Assert.Equal(("complete", (Guid?)user), tickets.Poll(first.Id, first.Secret));
        Assert.Equal("expired", tickets.Poll(first.Id, first.Secret).Status);
        for (var i = 0; i < 256; i++) { Assert.NotNull(tickets.Start()); }
        Assert.Null(tickets.Start()); clock.Now += TimeSpan.FromMinutes(6);
        Assert.NotNull(tickets.Start());
        Assert.Equal("expired", tickets.Poll(first.Id, first.Secret).Status);
    }
    [Fact]
    public void Oidc_configuration_requires_https_and_explicit_confidential_client_and_bootstrap_settings()
    {
        foreach (var invalid in new[] { "Auth:Oidc:Authority", "Auth:Oidc:PublicOrigin", "Auth:Oidc:ClientSecret", "Auth:Oidc:ClientId", "Bootstrap:OidcSubject" })
        {
            var builder = Microsoft.AspNetCore.Builder.WebApplication.CreateBuilder(new Microsoft.AspNetCore.Builder.WebApplicationOptions { EnvironmentName = "Testing" });
            var config = new Dictionary<string, string?>
            {
                ["Auth:Oidc:Authority"] = FakeOidcProvider.Issuer,
                ["Auth:Oidc:PublicOrigin"] = "https://localhost",
                ["Auth:Oidc:ClientId"] = "synthetic",
                ["Auth:Oidc:ClientSecret"] = "Synthetic-secret",
                ["Bootstrap:OidcSubject"] = "synthetic-admin"
            };
            config[invalid] = invalid.EndsWith("Authority") || invalid.EndsWith("PublicOrigin") ? "http://insecure.synthetic.test" : "";
            Microsoft.Extensions.Configuration.MemoryConfigurationBuilderExtensions.AddInMemoryCollection(builder.Configuration, config);
            Assert.Throws<InvalidOperationException>(() => OidcLogin.Configure(builder));
        }
    }

    private sealed class TestClock : TimeProvider
    {
        public DateTimeOffset Now = DateTimeOffset.UtcNow;
        public override DateTimeOffset GetUtcNow() => Now;
    }
}

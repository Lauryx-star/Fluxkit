using System.Net;
using System.Net.Http.Headers;
using System.Net.Http.Json;
using System.Text.Json;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Fluxkit.Server.Tests;

/// Desktop sessions: rotating refresh tokens keep a used client signed in; reuse, logout,
/// inactivity and disabled accounts end the session.
public sealed class SessionTests(ServerFixture server) : IClassFixture<ServerFixture>
{
    private const string Password = "SyntheticUser42!password";

    private async Task<(Guid Id, JsonElement Reply)> SignIn()
    {
        var user = await server.Register();
        using var admin = await server.Admin();
        Assert.Equal(HttpStatusCode.NoContent,
            (await admin.PostAsync($"/api/admin/users/{user.Id}/approve", null)).StatusCode);
        using var client = server.CreateClient();
        var response = await client.PostAsJsonAsync("/api/auth/login", new { username = user.Username, password = Password });
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        return (user.Id, await response.Content.ReadFromJsonAsync<JsonElement>());
    }

    private async Task<HttpResponseMessage> Refresh(string? token)
    {
        using var client = server.CreateClient();
        return await client.PostAsJsonAsync("/api/auth/refresh", new { refreshToken = token });
    }

    private async Task<HttpStatusCode> Account(string accessToken)
    {
        using var client = server.CreateClient();
        client.DefaultRequestHeaders.Authorization = new AuthenticationHeaderValue("Bearer", accessToken);
        return (await client.GetAsync("/api/account")).StatusCode;
    }

    [Fact]
    public async Task Sign_in_returns_a_short_access_token_and_a_seven_day_rotating_refresh_token()
    {
        var (_, reply) = await SignIn();
        Assert.Equal(3600, reply.GetProperty("expiresIn").GetInt32());
        var lifetime = reply.GetProperty("refreshExpiresIn").GetInt64();
        Assert.InRange(lifetime, TimeSpan.FromDays(7).TotalSeconds - 60, TimeSpan.FromDays(7).TotalSeconds);
        var first = reply.GetProperty("refreshToken").GetString()!;

        var response = await Refresh(first);
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.Equal("no-store", response.Headers.CacheControl?.ToString());
        var next = await response.Content.ReadFromJsonAsync<JsonElement>();
        var second = next.GetProperty("refreshToken").GetString()!;
        Assert.NotEqual(first, second);
        Assert.Equal(HttpStatusCode.OK, await Account(next.GetProperty("accessToken").GetString()!));

        // Reusing the rotated token ends the whole session, including the newer token.
        Assert.Equal(HttpStatusCode.Unauthorized, (await Refresh(first)).StatusCode);
        Assert.Equal(HttpStatusCode.Unauthorized, (await Refresh(second)).StatusCode);
    }

    [Fact]
    public async Task Logout_inactivity_and_disabled_accounts_end_the_session()
    {
        var (_, loggedOut) = await SignIn();
        using (var client = server.CreateClient())
        {
            Assert.Equal(HttpStatusCode.NoContent, (await client.PostAsJsonAsync("/api/auth/logout",
                new { refreshToken = loggedOut.GetProperty("refreshToken").GetString() })).StatusCode);
        }
        Assert.Equal(HttpStatusCode.Unauthorized, (await Refresh(loggedOut.GetProperty("refreshToken").GetString())).StatusCode);

        var (idleUser, idle) = await SignIn();
        using (var scope = server.Services.CreateScope())
        {
            var db = scope.ServiceProvider.GetRequiredService<FluxkitDb>();
            await db.RefreshSessions.Where(session => session.UserId == idleUser)
                .ExecuteUpdateAsync(set => set.SetProperty(session => session.ExpiresAt, DateTimeOffset.UtcNow.AddMinutes(-1)));
        }
        Assert.Equal(HttpStatusCode.Unauthorized, (await Refresh(idle.GetProperty("refreshToken").GetString())).StatusCode);

        var (disabledUser, disabled) = await SignIn();
        using (var admin = await server.Admin())
        {
            Assert.Equal(HttpStatusCode.NoContent, (await admin.PutAsJsonAsync(
                $"/api/admin/users/{disabledUser}/status", new { status = "disabled" })).StatusCode);
        }
        Assert.Equal(HttpStatusCode.Unauthorized, (await Refresh(disabled.GetProperty("refreshToken").GetString())).StatusCode);
        using (var scope = server.Services.CreateScope())
        {
            var db = scope.ServiceProvider.GetRequiredService<FluxkitDb>();
            Assert.False(await db.RefreshSessions.AnyAsync(session => session.UserId == disabledUser));
        }
    }

    [Theory]
    [InlineData(null)]
    [InlineData("")]
    [InlineData("not-a-token")]
    [InlineData("00000000000000000000000000000000.AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA")]
    public async Task Malformed_or_unknown_refresh_tokens_are_rejected(string? token)
    {
        Assert.Equal(HttpStatusCode.Unauthorized, (await Refresh(token)).StatusCode);
    }
}

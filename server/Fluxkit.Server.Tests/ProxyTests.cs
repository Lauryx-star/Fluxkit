using System.Net;
using System.Net.Http.Json;
using Microsoft.AspNetCore.Builder;
using Microsoft.AspNetCore.Hosting;
using Microsoft.AspNetCore.Mvc.Testing;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Fluxkit.Server.Tests;

/// Behind nginx, rate limits apply per client address taken from X-Forwarded-For, but only when
/// the request comes from a configured trusted proxy network.
public sealed class ProxyTests(ServerFixture server) : IClassFixture<ServerFixture>
{
    /// TestServer connections have no address; this stands in for the proxy's connection.
    private sealed class RemoteAddress(IPAddress address) : IStartupFilter
    {
        public Action<IApplicationBuilder> Configure(Action<IApplicationBuilder> next) => app =>
        {
            app.Use((context, following) =>
            {
                context.Connection.RemoteIpAddress = address;
                return following(context);
            });
            next(app);
        };
    }

    private WebApplicationFactory<Program> Behind(string remote)
    {
        var previous = Environment.GetEnvironmentVariable("Proxy__TrustedNetworks");
        try
        {
            Environment.SetEnvironmentVariable("Proxy__TrustedNetworks", "172.16.0.0/12");
            var factory = server.WithWebHostBuilder(builder => builder.ConfigureServices(services =>
                services.AddSingleton<IStartupFilter>(new RemoteAddress(IPAddress.Parse(remote)))));
            using var client = factory.CreateClient(); // Build while the setting is applied.
            return factory;
        }
        finally { Environment.SetEnvironmentVariable("Proxy__TrustedNetworks", previous); }
    }

    private static async Task<HttpStatusCode> FailedLogin(HttpClient client, string forwardedFor)
    {
        using var request = new HttpRequestMessage(HttpMethod.Post, "/api/auth/login")
        {
            Content = JsonContent.Create(new { username = "nobody-" + Guid.NewGuid().ToString("N"), password = "wrong" })
        };
        request.Headers.Add("X-Forwarded-For", forwardedFor);
        return (await client.SendAsync(request)).StatusCode;
    }

    [Fact]
    public async Task Trusted_proxies_rate_limit_each_forwarded_client_separately()
    {
        using var factory = Behind("172.18.0.1");
        using var client = factory.CreateClient();
        for (var attempt = 0; attempt < 10; attempt++)
        {
            Assert.Equal(HttpStatusCode.Unauthorized, await FailedLogin(client, "203.0.113.7, 198.51.100.1"));
        }
        Assert.Equal(HttpStatusCode.TooManyRequests, await FailedLogin(client, "198.51.100.1"));
        // Only the last hop counts: a client-supplied earlier entry cannot pick another bucket.
        Assert.Equal(HttpStatusCode.TooManyRequests, await FailedLogin(client, "198.51.100.9, 198.51.100.1"));
        Assert.Equal(HttpStatusCode.Unauthorized, await FailedLogin(client, "198.51.100.2"));
    }

    [Fact]
    public async Task Forwarded_addresses_from_untrusted_senders_are_ignored()
    {
        using var factory = Behind("203.0.113.50");
        using var client = factory.CreateClient();
        for (var attempt = 0; attempt < 10; attempt++)
        {
            Assert.Equal(HttpStatusCode.Unauthorized, await FailedLogin(client, $"198.51.100.{attempt + 10}"));
        }
        Assert.Equal(HttpStatusCode.TooManyRequests, await FailedLogin(client, "198.51.100.200"));
    }
}

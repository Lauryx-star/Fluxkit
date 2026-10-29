using System.Net;
using System.Net.Http.Json;
using System.Text;
using System.Text.Json;
using System.Text.RegularExpressions;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Fluxkit.Server.Tests;

/// Prometheus metrics: runtime/HTTP metrics and aggregated usage statistics, free of identifiers.
public sealed partial class MetricsTests(ServerFixture server) : IClassFixture<ServerFixture>
{
    private async Task<string> Scrape()
    {
        using var stream = new MemoryStream();
        await server.Services.GetRequiredService<UsageMetrics>().Registry.CollectAndExportAsTextAsync(stream);
        return Encoding.UTF8.GetString(stream.ToArray());
    }

    private static double Value(string text, string series)
    {
        var line = text.Split('\n').FirstOrDefault(line => line.StartsWith(series + " ", StringComparison.Ordinal));
        return line is null ? 0 : double.Parse(line[(series.Length + 1)..], System.Globalization.CultureInfo.InvariantCulture);
    }

    [GeneratedRegex("[0-9a-fA-F]{8}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{4}-[0-9a-fA-F]{12}")]
    private static partial Regex Uuid();

    [Fact]
    public async Task Usage_runtime_and_http_metrics_are_exported_without_identifiers()
    {
        var before = await Scrape();
        using var admin = await server.Admin();
        using var member = await server.ApprovedUser();
        Assert.Equal(HttpStatusCode.OK, (await member.GetAsync("/api/account")).StatusCode);
        Assert.Equal(HttpStatusCode.Unauthorized, (await server.CreateClient().PostAsJsonAsync("/api/auth/login",
            new { username = "testadmin", password = "Wrong-synthetic-password-1" })).StatusCode);
        var created = await admin.PostAsJsonAsync("/api/projects", new { name = "Metrics team" });
        var project = (await created.Content.ReadFromJsonAsync<JsonElement>()).GetProperty("id").GetGuid();
        var collection = Guid.NewGuid();
        var put = new { op = "put", kind = "collection", id = collection, data = new { name = "API", position = "g" } };
        Assert.Equal(HttpStatusCode.OK, (await admin.PostAsJsonAsync($"/api/projects/{project}/changes", new { operations = new[] { put } })).StatusCode);
        var stale = new { op = "put", kind = "collection", id = collection, expectedRevision = 7, data = new { name = "API", position = "g" } };
        Assert.Equal(HttpStatusCode.Conflict, (await admin.PostAsJsonAsync($"/api/projects/{project}/changes", new { operations = new[] { stale } })).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await admin.GetAsync($"/api/projects/{project}/changes?since=0")).StatusCode);

        var after = await Scrape();
        Assert.True(Value(after, "fluxkit_logins_total{mode=\"local\",result=\"success\"}") >= Value(before, "fluxkit_logins_total{mode=\"local\",result=\"success\"}") + 2);
        Assert.True(Value(after, "fluxkit_logins_total{mode=\"local\",result=\"rejected\"}") >= 1);
        Assert.True(Value(after, "fluxkit_change_batches_total{result=\"applied\"}") >= 1);
        Assert.True(Value(after, "fluxkit_change_batches_total{result=\"conflict\"}") >= 1);
        Assert.True(Value(after, "fluxkit_change_operations_total{kind=\"collection\",operation=\"put\"}") >= 1);
        Assert.True(Value(after, "fluxkit_projects_created_total{kind=\"team\",source=\"blank\"}") >= 1);
        Assert.True(Value(after, "fluxkit_feed_reads_total{result=\"changes\"}") >= 1);
        Assert.True(Value(after, "fluxkit_accounts{status=\"active\"}") >= 2);
        Assert.True(Value(after, "fluxkit_projects{kind=\"team\"}") >= 1);
        Assert.True(Value(after, "fluxkit_project_rows{kind=\"collection\"}") >= 1);
        Assert.True(Value(after, "fluxkit_active_users{window=\"5m\"}") >= 2);
        Assert.Contains("process_cpu_seconds_total", after);
        Assert.Contains("dotnet_collection_count_total", after);
        Assert.Contains("http_requests_received_total{", after);
        Assert.Contains("endpoint=\"/api/projects/{id:guid}/changes\"", after);
        Assert.DoesNotMatch(Uuid(), after);
        Assert.DoesNotContain("Metrics team", after);
        Assert.DoesNotContain("testadmin", after);
    }

    [Fact]
    public async Task Metrics_are_served_anonymously_in_the_prometheus_text_format()
    {
        using var admin = await server.Admin();
        var response = await server.CreateClient().GetAsync("/metrics");
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        Assert.StartsWith("text/plain", response.Content.Headers.ContentType?.MediaType);
        Assert.Contains("fluxkit_accounts{status=\"active\"}", await response.Content.ReadAsStringAsync());
    }
}

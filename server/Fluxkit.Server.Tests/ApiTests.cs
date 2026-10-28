using System.Net;
using System.Net.Http.Json;
using System.Text.Json;
using Xunit;

namespace Fluxkit.Server.Tests;

/// Entity-level project API: tree, batches with per-entity revisions, change feed, value
/// slots, upload and deleting empty projects.
public sealed class ApiTests(ServerFixture server) : IClassFixture<ServerFixture>
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);

    private async Task<(HttpClient Admin, HttpClient Member, Guid Project)> TeamProject()
    {
        var admin = await server.Admin();
        var member = await server.ApprovedUser();
        var account = await member.GetFromJsonAsync<JsonElement>("/api/account");
        var created = await admin.PostAsJsonAsync("/api/projects", new { name = "Team " + Guid.NewGuid().ToString("N")[..8] });
        Assert.Equal(HttpStatusCode.Created, created.StatusCode);
        var id = (await created.Content.ReadFromJsonAsync<JsonElement>()).GetProperty("id").GetGuid();
        Assert.Equal(HttpStatusCode.NoContent, (await admin.PostAsJsonAsync($"/api/projects/{id}/members",
            new { username = account.GetProperty("userName").GetString(), role = "member" })).StatusCode);
        return (admin, member, id);
    }

    private static object Put(string kind, Guid id, long? expected, object data) =>
        new { op = "put", kind, id, expectedRevision = expected, data };
    private static object Delete(string kind, Guid id, long expected) =>
        new { op = "delete", kind, id, expectedRevision = expected };
    private static object Collection(string name, string position = "g") => new { name, position };
    private static object Folder(Guid collection, Guid? parent, string name = "Folder", string position = "g") =>
        new { collectionId = collection, parentId = parent, name, position };
    private static object Request(Guid collection, Guid? folder, string url, string position = "g") => new
    {
        collectionId = collection,
        folderId = folder,
        name = "Request",
        method = "GET",
        url,
        details = new { @params = Array.Empty<object>(), headers = Array.Empty<object>(), body = new { kind = "none", text = "", fields = Array.Empty<object>() } },
        position
    };

    private static Task<HttpResponseMessage> Batch(HttpClient client, Guid project, params object[] operations) =>
        client.PostAsJsonAsync($"/api/projects/{project}/changes", new { operations });

    private static async Task<JsonElement> Body(HttpResponseMessage response) =>
        await response.Content.ReadFromJsonAsync<JsonElement>(Json);

    private static Task<HttpResponseMessage> WithRevision(HttpClient client, HttpMethod method, string path, long revision, object? body = null)
    {
        var request = new HttpRequestMessage(method, path);
        if (body is not null) { request.Content = JsonContent.Create(body); }
        request.Headers.TryAddWithoutValidation("If-Match", $"\"{revision}\"");
        return client.SendAsync(request);
    }

    /// One collection with two top-level requests.
    private static async Task<(Guid Collection, Guid First, Guid Second)> Seed(HttpClient client, Guid project)
    {
        var collection = Guid.NewGuid(); var first = Guid.NewGuid(); var second = Guid.NewGuid();
        var response = await Batch(client, project,
            Put("collection", collection, null, Collection("API")),
            Put("request", first, null, Request(collection, null, "https://synthetic.invalid/one", "g")),
            Put("request", second, null, Request(collection, null, "https://synthetic.invalid/two", "n")));
        Assert.Equal(HttpStatusCode.OK, response.StatusCode);
        var revisions = (await Body(response)).GetProperty("revisions");
        Assert.Equal(1, revisions.GetProperty(first.ToString()).GetInt64());
        return (collection, first, second);
    }

    [Fact]
    public async Task Two_members_editing_different_requests_both_succeed()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var (collection, first, second) = await Seed(admin, project);
        var results = await Task.WhenAll(
            Batch(admin, project, Put("request", first, 1, Request(collection, null, "https://synthetic.invalid/admin", "g"))),
            Batch(member, project, Put("request", second, 1, Request(collection, null, "https://synthetic.invalid/member", "n"))));
        Assert.All(results, response => Assert.Equal(HttpStatusCode.OK, response.StatusCode));
        var tree = await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/tree");
        var urls = tree.GetProperty("requests").EnumerateArray().Select(item => item.GetProperty("url").GetString()).ToHashSet();
        Assert.Contains("https://synthetic.invalid/admin", urls);
        Assert.Contains("https://synthetic.invalid/member", urls);
    }

    [Fact]
    public async Task A_stale_revision_is_a_conflict_that_names_who_changed_it_and_writes_nothing()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var (collection, first, second) = await Seed(admin, project);
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project, Put("request", first, 1, Request(collection, null, "https://synthetic.invalid/winner")))).StatusCode);
        var stale = await Batch(member, project,
            Put("request", second, 1, Request(collection, null, "https://synthetic.invalid/must-not-land", "n")),
            Put("request", first, 1, Request(collection, null, "https://synthetic.invalid/loser")));
        Assert.Equal(HttpStatusCode.Conflict, stale.StatusCode);
        var conflict = (await Body(stale)).GetProperty("conflicts").EnumerateArray().Single();
        Assert.Equal(first, conflict.GetProperty("id").GetGuid());
        Assert.Equal(2, conflict.GetProperty("revision").GetInt64());
        Assert.Equal("testadmin", conflict.GetProperty("updatedBy").GetString());
        var tree = await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/tree");
        Assert.DoesNotContain(tree.GetProperty("requests").EnumerateArray(), item => item.GetProperty("url").GetString() == "https://synthetic.invalid/must-not-land");
    }

    [Fact]
    public async Task Concurrent_writers_of_the_same_request_accept_exactly_one()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var (collection, first, _) = await Seed(admin, project);
        var results = await Task.WhenAll(Enumerable.Range(0, 4).Select(i =>
            Batch(i % 2 == 0 ? admin : member, project, Put("request", first, 1, Request(collection, null, $"https://synthetic.invalid/{i}")))));
        Assert.Single(results, response => response.StatusCode == HttpStatusCode.OK);
        Assert.Equal(3, results.Count(response => response.StatusCode == HttpStatusCode.Conflict));
    }

    [Fact]
    public async Task Tree_rules_reject_cycles_foreign_parents_and_unknown_collections()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var (collection, _, _) = await Seed(admin, project);
        var a = Guid.NewGuid(); var b = Guid.NewGuid();
        Assert.Equal(HttpStatusCode.BadRequest, (await Batch(admin, project,
            Put("folder", a, null, Folder(collection, b)), Put("folder", b, null, Folder(collection, a)))).StatusCode);
        var other = Guid.NewGuid(); var foreign = Guid.NewGuid();
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project,
            Put("collection", other, null, Collection("Other", "n")), Put("folder", foreign, null, Folder(other, null)))).StatusCode);
        Assert.Equal(HttpStatusCode.BadRequest, (await Batch(admin, project,
            Put("request", Guid.NewGuid(), null, Request(collection, foreign, "https://synthetic.invalid/x")))).StatusCode);
        Assert.Equal(HttpStatusCode.BadRequest, (await Batch(admin, project,
            Put("folder", Guid.NewGuid(), null, Folder(Guid.NewGuid(), null)))).StatusCode);
        Assert.Equal(HttpStatusCode.BadRequest, (await Batch(admin, project,
            Put("collection", Guid.NewGuid(), null, new { name = "Bad position", position = "UPPER" }))).StatusCode);
    }

    [Fact]
    public async Task Moving_and_deleting_cascade_like_the_client_expects()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var (collection, first, second) = await Seed(admin, project);
        var folder = Guid.NewGuid(); var child = Guid.NewGuid();
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project,
            Put("folder", folder, null, Folder(collection, null)),
            Put("folder", child, null, Folder(collection, folder, "Child")),
            Put("request", first, 1, Request(collection, child, "https://synthetic.invalid/one")))).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project, Delete("folder", folder, 1))).StatusCode);
        var tree = await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/tree");
        Assert.Empty(tree.GetProperty("folders").EnumerateArray());
        Assert.Equal(second, tree.GetProperty("requests").EnumerateArray().Single().GetProperty("id").GetGuid());
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project, Delete("collection", collection, 1))).StatusCode);
        tree = await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/tree");
        Assert.Empty(tree.GetProperty("requests").EnumerateArray());
    }

    [Fact]
    public async Task The_change_feed_reports_puts_and_deletes_since_a_sequence_number()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var start = (await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/tree")).GetProperty("seq").GetInt64();
        var (collection, first, second) = await Seed(admin, project);
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project, Delete("request", second, 1))).StatusCode);
        var feed = await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/changes?since={start}");
        var changes = feed.GetProperty("changes").EnumerateArray().ToList();
        Assert.Contains(changes, change => change.GetProperty("id").GetGuid() == collection && change.GetProperty("operation").GetString() == "put");
        Assert.Contains(changes, change => change.GetProperty("id").GetGuid() == first && change.GetProperty("data").GetProperty("url").GetString() == "https://synthetic.invalid/one");
        Assert.Contains(changes, change => change.GetProperty("id").GetGuid() == second && change.GetProperty("operation").GetString() == "delete");
        var seq = feed.GetProperty("seq").GetInt64();
        Assert.Empty((await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/changes?since={seq}")).GetProperty("changes").EnumerateArray());
        Assert.Equal(HttpStatusCode.Gone, (await member.GetAsync($"/api/projects/{project}/changes?since=-1")).StatusCode);
    }

    [Fact]
    public async Task A_client_polling_the_feed_during_parallel_batches_misses_no_change()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var (collection, _, _) = await Seed(admin, project);
        var start = (await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/tree")).GetProperty("seq").GetInt64();
        var created = Enumerable.Range(0, 24).Select(_ => Guid.NewGuid()).ToList();
        var seen = new HashSet<Guid>();
        var since = start;
        var writers = Task.WhenAll(created.Select((id, i) => Batch(i % 2 == 0 ? admin : member, project,
            Put("request", id, null, Request(collection, null, $"https://synthetic.invalid/parallel/{i}", "t" + (char)('a' + i))))));
        while (!writers.IsCompleted)
        {
            var feed = await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/changes?since={since}");
            foreach (var change in feed.GetProperty("changes").EnumerateArray()) { seen.Add(change.GetProperty("id").GetGuid()); }
            since = feed.GetProperty("seq").GetInt64();
        }
        Assert.All(await writers, response => Assert.Equal(HttpStatusCode.OK, response.StatusCode));
        var last = await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/changes?since={since}");
        foreach (var change in last.GetProperty("changes").EnumerateArray()) { seen.Add(change.GetProperty("id").GetGuid()); }
        Assert.All(created, id => Assert.Contains(id, seen));
    }

    [Fact]
    public async Task Strangers_can_neither_read_nor_write_and_personal_projects_stay_private()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        using var stranger = await server.ApprovedUser();
        Assert.Equal(HttpStatusCode.NotFound, (await stranger.GetAsync($"/api/projects/{project}/tree")).StatusCode);
        Assert.Equal(HttpStatusCode.NotFound, (await Batch(stranger, project, Put("collection", Guid.NewGuid(), null, Collection("X")))).StatusCode);
        var personal = (await ServerFixture.PersonalProject(member)).GetProperty("id").GetGuid();
        Assert.Equal(HttpStatusCode.NotFound, (await admin.GetAsync($"/api/projects/{personal}/tree")).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await Batch(member, personal, Put("collection", Guid.NewGuid(), null, Collection("Mine")))).StatusCode);
    }

    [Fact]
    public async Task Variable_names_stay_unique_and_values_follow_the_classification()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        var ordinary = Guid.NewGuid(); var secret = Guid.NewGuid(); var environment = Guid.NewGuid();
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project,
            Put("environment", environment, null, new { name = "Test", position = "g" }),
            Put("variable", ordinary, null, new { name = "baseUrl", secret = false, position = "g" }),
            Put("variable", secret, null, new { name = "token", secret = true, position = "n" }))).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await Batch(admin, project,
            Put("variable", Guid.NewGuid(), null, new { name = "baseUrl", secret = false, position = "u" }))).StatusCode);

        // Different slots never conflict; the same slot is revision-checked.
        Assert.Equal(HttpStatusCode.OK, (await WithRevision(admin, HttpMethod.Put, $"/api/projects/{project}/values/{ordinary}/default", 0, new { value = "https://synthetic.invalid" })).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await WithRevision(member, HttpMethod.Put, $"/api/projects/{project}/values/{ordinary}/{environment}", 0, new { value = "https://test.synthetic.invalid" })).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await WithRevision(member, HttpMethod.Put, $"/api/projects/{project}/values/{ordinary}/default", 0, new { value = "stale" })).StatusCode);
        var updated = await WithRevision(member, HttpMethod.Put, $"/api/projects/{project}/values/{ordinary}/default", 1, new { value = "https://new.synthetic.invalid" });
        Assert.Equal(2, (await Body(updated)).GetProperty("revision").GetInt64());
        Assert.Equal(HttpStatusCode.BadRequest, (await WithRevision(admin, HttpMethod.Put, $"/api/projects/{project}/values/{secret}/default", 0, new { value = "must stay secret" })).StatusCode);
        Assert.Equal(HttpStatusCode.BadRequest, (await WithRevision(admin, HttpMethod.Put, $"/api/projects/{project}/values/{ordinary}/{Guid.NewGuid()}", 0, new { value = "unknown environment" })).StatusCode);
        Assert.Equal(2, (await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/values")).GetProperty("values").GetArrayLength());

        // Vault rows are per user and require a vault profile.
        var cipher = new { nonce = Convert.ToBase64String(new byte[12]), ciphertext = Convert.ToBase64String(new byte[16]) };
        Assert.Equal(HttpStatusCode.BadRequest, (await WithRevision(member, HttpMethod.Put, $"/api/projects/{project}/vault-values/{secret}/default", 0, cipher)).StatusCode);
        var profile = new { profile = new { schemaVersion = 1, algorithm = "AES-256-GCM", kdf = new { algorithm = "Argon2id", version = 19, memoryKiB = 65536, iterations = 3, parallelism = 1 }, salt = Convert.ToBase64String(new byte[16]), nonce = Convert.ToBase64String(new byte[12]), wrappedKey = Convert.ToBase64String(new byte[48]) } };
        Assert.Equal(HttpStatusCode.OK, (await WithRevision(member, HttpMethod.Put, "/api/account/vault", 0, profile)).StatusCode);
        Assert.Equal(HttpStatusCode.OK, (await WithRevision(member, HttpMethod.Put, $"/api/projects/{project}/vault-values/{secret}/default", 0, cipher)).StatusCode);
        Assert.Single((await member.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/vault-values")).GetProperty("values").EnumerateArray());
        Assert.Empty((await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/vault-values")).GetProperty("values").EnumerateArray());

        // Reclassifying drops values of the old kind; deleting an environment drops its slots.
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project, Put("variable", ordinary, 1, new { name = "baseUrl", secret = true, position = "g" }))).StatusCode);
        Assert.Empty((await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/values")).GetProperty("values").EnumerateArray());
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project, Delete("environment", environment, 1))).StatusCode);
    }

    [Fact]
    public async Task Uploading_a_local_project_creates_its_tree_and_values_atomically()
    {
        using var owner = await server.ApprovedUser();
        var id = Guid.NewGuid(); var collection = Guid.NewGuid(); var folder = Guid.NewGuid(); var child = Guid.NewGuid();
        var variable = Guid.NewGuid();
        object Tree(object? sharedValue) => new
        {
            name = "Uploaded",
            id,
            kind = "personal",
            tree = new
            {
                collections = new[] { new { id = collection, data = Collection("API") } },
                folders = new[] { new { id = child, data = Folder(collection, folder, "Child") }, new { id = folder, data = Folder(collection, null, "Parent") } },
                requests = new[] { new { id = Guid.NewGuid(), data = Request(collection, child, "https://synthetic.invalid") } },
                variables = new[] { new { id = variable, data = (object)new { name = "baseUrl", secret = false, position = "g" } } }
            },
            sharedValues = sharedValue is null ? null : new[] { sharedValue }
        };
        Assert.Equal(HttpStatusCode.BadRequest, (await owner.PostAsJsonAsync("/api/projects", Tree(new { variableId = variable, environmentId = "missing", value = "x" }))).StatusCode);
        Assert.Equal(HttpStatusCode.NotFound, (await owner.GetAsync($"/api/projects/{id}/tree")).StatusCode);
        Assert.Equal(HttpStatusCode.Created, (await owner.PostAsJsonAsync("/api/projects", Tree(new { variableId = variable, environmentId = "default", value = "https://synthetic.invalid" }))).StatusCode);
        var tree = await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/tree");
        Assert.Equal(2, tree.GetProperty("folders").GetArrayLength());
        Assert.Single(tree.GetProperty("requests").EnumerateArray());
        Assert.Single((await owner.GetFromJsonAsync<JsonElement>($"/api/projects/{id}/values")).GetProperty("values").EnumerateArray());
        Assert.Equal(HttpStatusCode.Conflict, (await owner.PostAsJsonAsync("/api/projects", Tree(null))).StatusCode);
        Assert.Equal(HttpStatusCode.Forbidden, (await owner.PostAsJsonAsync("/api/projects", new { name = "Not an admin" })).StatusCode);
    }

    [Fact]
    public async Task Only_server_admins_can_delete_and_only_empty_projects()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        Assert.Equal(HttpStatusCode.Forbidden, (await WithRevision(member, HttpMethod.Delete, $"/api/projects/{project}", 1)).StatusCode);
        var (collection, _, _) = await Seed(admin, project);
        var blocked = await WithRevision(admin, HttpMethod.Delete, $"/api/projects/{project}", 1);
        Assert.Equal(HttpStatusCode.Conflict, blocked.StatusCode);
        Assert.Equal(1, (await Body(blocked)).GetProperty("collections").GetInt32());
        Assert.Equal(HttpStatusCode.OK, (await Batch(admin, project, Delete("collection", collection, 1))).StatusCode);
        Assert.Equal((HttpStatusCode)428, (await admin.DeleteAsync($"/api/projects/{project}")).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await WithRevision(admin, HttpMethod.Delete, $"/api/projects/{project}", 7)).StatusCode);
        Assert.Equal(HttpStatusCode.NoContent, (await WithRevision(admin, HttpMethod.Delete, $"/api/projects/{project}", 1)).StatusCode);
        Assert.Equal(HttpStatusCode.NotFound, (await admin.GetAsync($"/api/projects/{project}/tree")).StatusCode);
        var list = await admin.GetFromJsonAsync<JsonElement>("/api/projects");
        Assert.DoesNotContain(list.EnumerateArray(), item => item.GetProperty("id").GetGuid() == project);
    }

    [Fact]
    public async Task The_project_list_shows_creation_date_and_collection_count()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        await Seed(admin, project);
        var row = (await member.GetFromJsonAsync<JsonElement>("/api/projects")).EnumerateArray().Single(item => item.GetProperty("id").GetGuid() == project);
        Assert.Equal(1, row.GetProperty("collectionCount").GetInt32());
        Assert.True(row.GetProperty("createdAt").GetDateTimeOffset() > DateTimeOffset.UtcNow.AddMinutes(-5));
    }

    [Fact]
    public async Task Project_name_and_auth_change_with_the_project_revision()
    {
        var (admin, member, project) = await TeamProject();
        using var _ = admin; using var __ = member;
        Assert.Equal(HttpStatusCode.OK, (await Batch(member, project, Put("project", project, 1, new { name = "Renamed", auth = "none" }))).StatusCode);
        Assert.Equal(HttpStatusCode.Conflict, (await Batch(admin, project, Put("project", project, 1, new { name = "Stale" }))).StatusCode);
        var tree = await admin.GetFromJsonAsync<JsonElement>($"/api/projects/{project}/tree");
        Assert.Equal("Renamed", tree.GetProperty("project").GetProperty("name").GetString());
        Assert.Equal(2, tree.GetProperty("project").GetProperty("revision").GetInt64());
    }
}

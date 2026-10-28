using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text.Json;
using Microsoft.EntityFrameworkCore;

namespace Fluxkit.Server;

public sealed record TreeItem(Guid Id, JsonElement Data);
public sealed record ProjectTreeInput(
    JsonElement? Auth, List<TreeItem>? Collections, List<TreeItem>? Folders, List<TreeItem>? Requests,
    List<TreeItem>? Environments, List<TreeItem>? Variables);
public sealed record CreateProjectInput(
    string Name, Guid? Id = null, string? Kind = null, ProjectTreeInput? Tree = null,
    List<SharedSlotInput>? SharedValues = null, List<VaultSlotInput>? VaultValues = null);

/// Normalized project storage: the whole tree, the change feed and atomic entity batches.
public static class ProjectEndpoints
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    private static Guid User(ClaimsPrincipal caller) => Guid.Parse(caller.FindFirstValue(JwtRegisteredClaimNames.Sub)!);

    public static Task<ProjectRecord?> Readable(FluxkitDb db, Guid id, Guid user, CancellationToken ct) =>
        db.Projects.SingleOrDefaultAsync(project => project.Id == id && (project.OwnerId == user
            || db.Memberships.Any(member => member.ProjectId == id && member.UserId == user)), ct);

    public static async Task<bool> Writable(FluxkitDb db, ProjectRecord project, Guid user, CancellationToken ct) =>
        project.Kind == "personal"
            ? project.OwnerId == user
            : await db.Memberships.AnyAsync(member => member.ProjectId == project.Id && member.UserId == user
                && (member.Role == "projectAdmin" || member.Role == "member"), ct);

    private static JsonElement? Element(string? json)
    {
        if (json is null) { return null; }
        using var document = JsonDocument.Parse(json);
        return document.RootElement.Clone();
    }

    private static object CollectionView(CollectionRecord item) => new { item.Id, item.Revision, item.Name, Auth = Element(item.Auth), OpenApi = Element(item.OpenApi), Postman = Element(item.Postman), item.Position };
    private static object FolderView(FolderRecord item) => new { item.Id, item.Revision, item.CollectionId, item.ParentId, item.Name, Auth = Element(item.Auth), item.Position };
    private static object RequestView(RequestRecord item) => new { item.Id, item.Revision, item.CollectionId, item.FolderId, item.Name, item.Method, item.Url, Details = Element(item.Details), item.Position };
    private static object EnvironmentView(EnvironmentRecord item) => new { item.Id, item.Revision, item.Name, item.Position };
    private static object VariableView(VariableRecord item) => new { item.Id, item.Revision, item.Name, item.Secret, item.Position };

    private static async Task<long> Seq(FluxkitDb db, Guid project, CancellationToken ct) =>
        await db.ProjectChanges.Where(change => change.ProjectId == project).MaxAsync(change => (long?)change.Seq, ct) ?? 0;

    public static void Map(WebApplication app)
    {
        app.MapGet("/api/projects/{id:guid}/tree", async (Guid id, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            var project = await Readable(db, id, user, ct);
            if (project is null) { return Results.NotFound(); }
            // One snapshot-isolated read so the tree and its seq belong together.
            await using var transaction = await db.Database.BeginTransactionAsync(System.Data.IsolationLevel.RepeatableRead, ct);
            var seq = await Seq(db, id, ct);
            var result = new
            {
                Project = new { project.Id, project.Name, project.Kind, Auth = Element(project.Auth), project.Revision, CanEdit = await Writable(db, project, user, ct) },
                Collections = (await db.Collections.AsNoTracking().Where(item => item.ProjectId == id).ToListAsync(ct)).Select(CollectionView),
                Folders = (await db.Folders.AsNoTracking().Where(item => item.ProjectId == id).ToListAsync(ct)).Select(FolderView),
                Requests = (await db.Requests.AsNoTracking().Where(item => item.ProjectId == id).ToListAsync(ct)).Select(RequestView),
                Environments = (await db.Environments.AsNoTracking().Where(item => item.ProjectId == id).ToListAsync(ct)).Select(EnvironmentView),
                Variables = (await db.Variables.AsNoTracking().Where(item => item.ProjectId == id).ToListAsync(ct)).Select(VariableView),
                Seq = seq
            };
            await transaction.CommitAsync(ct);
            return Results.Ok(result);
        }).RequireAuthorization();

        app.MapGet("/api/projects/{id:guid}/changes", async (Guid id, long since, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var project = await Readable(db, id, User(caller), ct);
            if (project is null) { return Results.NotFound(); }
            if (since < 0) { return Results.StatusCode(StatusCodes.Status410Gone); }
            await using var transaction = await db.Database.BeginTransactionAsync(System.Data.IsolationLevel.RepeatableRead, ct);
            var seq = await Seq(db, id, ct);
            // Latest change per entity; the current row (or its absence) is what the client needs.
            var latest = await db.ProjectChanges.AsNoTracking()
                .Where(change => change.ProjectId == id && change.Seq > since)
                .GroupBy(change => new { change.EntityKind, change.EntityId })
                .Select(group => new { group.Key.EntityKind, group.Key.EntityId, Seq = group.Max(change => change.Seq) })
                .ToListAsync(ct);
            var ids = latest.Select(change => change.EntityId).ToHashSet();
            var collections = await db.Collections.AsNoTracking().Where(item => ids.Contains(item.Id) && item.ProjectId == id).ToDictionaryAsync(item => item.Id, ct);
            var folders = await db.Folders.AsNoTracking().Where(item => ids.Contains(item.Id) && item.ProjectId == id).ToDictionaryAsync(item => item.Id, ct);
            var requests = await db.Requests.AsNoTracking().Where(item => ids.Contains(item.Id) && item.ProjectId == id).ToDictionaryAsync(item => item.Id, ct);
            var environments = await db.Environments.AsNoTracking().Where(item => ids.Contains(item.Id) && item.ProjectId == id).ToDictionaryAsync(item => item.Id, ct);
            var variables = await db.Variables.AsNoTracking().Where(item => ids.Contains(item.Id) && item.ProjectId == id).ToDictionaryAsync(item => item.Id, ct);
            var changes = new List<object>();
            foreach (var change in latest.OrderBy(change => change.Seq))
            {
                (long Revision, object Data)? current = change.EntityKind switch
                {
                    "project" => (project.Revision, new { project.Name, Auth = Element(project.Auth) }),
                    "collection" when collections.TryGetValue(change.EntityId, out var item) => (item.Revision, CollectionView(item)),
                    "folder" when folders.TryGetValue(change.EntityId, out var item) => (item.Revision, FolderView(item)),
                    "request" when requests.TryGetValue(change.EntityId, out var item) => (item.Revision, RequestView(item)),
                    "environment" when environments.TryGetValue(change.EntityId, out var item) => (item.Revision, EnvironmentView(item)),
                    "variable" when variables.TryGetValue(change.EntityId, out var item) => (item.Revision, VariableView(item)),
                    _ => null
                };
                changes.Add(current is { } found
                    ? new { Kind = change.EntityKind, Id = change.EntityId, Operation = "put", found.Revision, Data = (object?)found.Data }
                    : new { Kind = change.EntityKind, Id = change.EntityId, Operation = "delete", Revision = 0L, Data = (object?)null });
            }
            await transaction.CommitAsync(ct);
            return Results.Ok(new { Seq = seq, Changes = changes });
        }).RequireAuthorization();

        app.MapPost("/api/projects/{id:guid}/changes", async (Guid id, ChangeBatch input, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            var project = await Readable(db, id, user, ct);
            if (project is null) { return Results.NotFound(); }
            if (!await Writable(db, project, user, ct)) { return Results.Forbid(); }
            if (input?.Operations is null) { return Results.BadRequest(new { error = "A batch needs operations." }); }
            return await Commit(db, project, user, input.Operations, ct);
        }).RequireAuthorization();

        app.MapPost("/api/projects", async (CreateProjectInput input, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            if (input is null || input.Name is null) { return Results.BadRequest(); }
            var name = input.Name.Trim();
            if (name.Length is < 1 or > 200 || name.Any(char.IsControl)) { return Results.BadRequest(new { error = "Project names must contain 1 to 200 characters." }); }
            var kind = input.Kind ?? "team";
            if (kind is not ("team" or "personal")) { return Results.BadRequest(); }
            // Team projects are created by server administrators. A personal project with a
            // given identity is the upload of a local project by its owner.
            if (kind == "team" && !caller.IsInRole("serverAdmin")) { return Results.Forbid(); }
            if (kind == "personal" && input.Id is null) { return Results.BadRequest(new { error = "Uploading a personal project requires its identity." }); }
            var id = input.Id ?? Guid.NewGuid();
            if (id == Guid.Empty) { return Results.BadRequest(); }
            if (await db.Projects.AnyAsync(project => project.Id == id, ct)) { return Results.Conflict(new { error = "Project identity already exists on this server." }); }
            await using var transaction = await db.Database.BeginTransactionAsync(ct);
            var project = new ProjectRecord { Id = id, OwnerId = user, Name = name, Kind = kind, Auth = ChangeEngine.Raw(input.Tree?.Auth), UpdatedBy = user };
            db.Projects.Add(project);
            if (kind == "team")
            {
                db.Memberships.Add(new ProjectMembership { ProjectId = id, UserId = user, Role = "projectAdmin" });
            }
            try { await db.SaveChangesAsync(ct); }
            catch (DbUpdateException) { return Results.Conflict(new { error = "Project identity already exists on this server." }); }
            if (input.Tree is { } tree)
            {
                var operations = Puts("collection", tree.Collections).Concat(FoldersParentFirst(tree.Folders))
                    .Concat(Puts("request", tree.Requests)).Concat(Puts("environment", tree.Environments))
                    .Concat(Puts("variable", tree.Variables)).ToList();
                var outcome = await ChangeEngine.ApplyAsync(db, project, user, operations, ct);
                if (outcome.Error is not null || outcome.Conflicts.Count > 0)
                {
                    return Results.BadRequest(new { error = outcome.Error ?? "The uploaded project contains duplicate identities." });
                }
                try { await db.SaveChangesAsync(ct); }
                catch (DbUpdateException) { return Results.BadRequest(new { error = "The uploaded project contains duplicate names or identities." }); }
                var limit = await ChangeEngine.CheckLimitsAsync(db, id, ct);
                if (limit is not null) { return Results.BadRequest(new { error = limit }); }
            }
            if ((input.SharedValues?.Count ?? 0) + (input.VaultValues?.Count ?? 0) > 0)
            {
                var values = await ValueEndpoints.ApplySlotsAsync(db, id, user, input.SharedValues ?? [], input.VaultValues ?? [], ct);
                if (values is not null) { return values; }
            }
            await transaction.CommitAsync(ct);
            return Results.Created($"/api/projects/{id}", new { project.Id, project.Name, project.Kind, project.Revision, project.CreatedAt });
        }).RequireAuthorization();

        app.MapDelete("/api/projects/{id:guid}", async (Guid id, ClaimsPrincipal caller, HttpRequest request, FluxkitDb db, CancellationToken ct) =>
        {
            if (!caller.IsInRole("serverAdmin")) { return Results.Forbid(); }
            var project = await db.Projects.SingleOrDefaultAsync(item => item.Id == id, ct);
            if (project is null) { return Results.NotFound(); }
            if (!long.TryParse(request.Headers.IfMatch.ToString().Trim('"'), out var expected)) { return Results.StatusCode(StatusCodes.Status428PreconditionRequired); }
            if (expected != project.Revision) { return Results.Conflict(new { error = "The project changed. Reload before deleting.", project.Revision }); }
            await using var transaction = await db.Database.BeginTransactionAsync(System.Data.IsolationLevel.Serializable, ct);
            var collections = await db.Collections.CountAsync(item => item.ProjectId == id, ct);
            var environments = await db.Environments.CountAsync(item => item.ProjectId == id, ct);
            var variables = await db.Variables.CountAsync(item => item.ProjectId == id, ct);
            if (collections + environments + variables > 0)
            {
                return Results.Conflict(new { error = "Only empty projects can be deleted. Remove its collections, environments and variables first.", collections, environments, variables });
            }
            var deleted = await db.Projects.Where(item => item.Id == id && item.Revision == expected).ExecuteDeleteAsync(ct);
            if (deleted == 0) { return Results.Conflict(new { error = "The project changed. Reload before deleting." }); }
            await transaction.CommitAsync(ct);
            return Results.NoContent();
        }).RequireAuthorization();
    }

    private static IEnumerable<ChangeOperation> Puts(string kind, List<TreeItem>? items) =>
        (items ?? []).Select(item => new ChangeOperation("put", kind, item.Id, null, item.Data));

    /// Upload order for folders: a parent always precedes its children.
    private static IEnumerable<ChangeOperation> FoldersParentFirst(List<TreeItem>? folders)
    {
        var pending = (folders ?? []).ToList();
        var placed = new HashSet<Guid>();
        var ids = pending.Select(item => item.Id).ToHashSet();
        while (pending.Count > 0)
        {
            var ready = pending.Where(item => !item.Data.TryGetProperty("parentId", out var parent)
                || parent.ValueKind != JsonValueKind.String || !parent.TryGetGuid(out var parentId)
                || placed.Contains(parentId) || !ids.Contains(parentId)).ToList();
            if (ready.Count == 0)
            {
                // A cycle: emit the rest unchanged; the engine rejects it.
                ready = pending;
            }
            foreach (var item in ready)
            {
                placed.Add(item.Id);
                yield return new ChangeOperation("put", "folder", item.Id, null, item.Data);
            }
            pending = pending.Except(ready).ToList();
        }
    }

    private static async Task<IResult> Commit(FluxkitDb db, ProjectRecord project, Guid user, List<ChangeOperation> operations, CancellationToken ct)
    {
        await using var transaction = await db.Database.BeginTransactionAsync(ct);
        // Batches of one project commit one after another, so feed sequence numbers become
        // visible in order: a reader that saw seq n never misses a later-committed seq below n.
        var key = project.Id.ToString();
        await db.Database.ExecuteSqlInterpolatedAsync($"SELECT pg_advisory_xact_lock(hashtextextended({key}, 0))", ct);
        var outcome = await ChangeEngine.ApplyAsync(db, project, user, operations, ct);
        if (outcome.Error is not null)
        {
            return Results.BadRequest(new { error = outcome.Error });
        }
        if (outcome.Conflicts.Count > 0)
        {
            await transaction.RollbackAsync(ct);
            return await ConflictResult(db, project.Id, outcome.Conflicts.Select(conflict => (conflict.Kind, conflict.Id)), ct);
        }
        try
        {
            await db.SaveChangesAsync(ct);
        }
        catch (DbUpdateConcurrencyException error)
        {
            // Another writer committed one of these rows between our read and our write.
            var stale = error.Entries.Select(entry => entry.Entity switch
            {
                ProjectRecord item => ("project", item.Id),
                CollectionRecord item => ("collection", item.Id),
                FolderRecord item => ("folder", item.Id),
                RequestRecord item => ("request", item.Id),
                EnvironmentRecord item => ("environment", item.Id),
                VariableRecord item => ("variable", item.Id),
                _ => ("", Guid.Empty)
            }).Where(item => item.Item2 != Guid.Empty).ToList();
            await transaction.RollbackAsync(ct);
            db.ChangeTracker.Clear();
            return await ConflictResult(db, project.Id, stale, ct);
        }
        catch (DbUpdateException)
        {
            await transaction.RollbackAsync(ct);
            return Results.Conflict(new { error = "A name is already used in this project, or an entity already exists.", conflicts = Array.Empty<object>() });
        }
        var limit = await ChangeEngine.CheckLimitsAsync(db, project.Id, ct);
        if (limit is not null)
        {
            await transaction.RollbackAsync(ct);
            return Results.BadRequest(new { error = limit });
        }
        await transaction.CommitAsync(ct);
        return Results.Ok(new { Seq = await Seq(db, project.Id, ct), outcome.Revisions });
    }

    /// 409 with the current state of each entity the client tried to change: its revision (or
    /// null when it is gone), when and by whom it was last changed.
    private static async Task<IResult> ConflictResult(FluxkitDb db, Guid project, IEnumerable<(string Kind, Guid Id)> entities, CancellationToken ct)
    {
        var conflicts = new List<object>();
        foreach (var (kind, id) in entities.Distinct())
        {
            var row = await (kind switch
            {
                "project" => db.Projects.AsNoTracking().Where(item => item.Id == id).Select(item => new { item.Revision, item.UpdatedAt, item.UpdatedBy }),
                "collection" => db.Collections.AsNoTracking().Where(item => item.Id == id && item.ProjectId == project).Select(item => new { item.Revision, item.UpdatedAt, item.UpdatedBy }),
                "folder" => db.Folders.AsNoTracking().Where(item => item.Id == id && item.ProjectId == project).Select(item => new { item.Revision, item.UpdatedAt, item.UpdatedBy }),
                "request" => db.Requests.AsNoTracking().Where(item => item.Id == id && item.ProjectId == project).Select(item => new { item.Revision, item.UpdatedAt, item.UpdatedBy }),
                "environment" => db.Environments.AsNoTracking().Where(item => item.Id == id && item.ProjectId == project).Select(item => new { item.Revision, item.UpdatedAt, item.UpdatedBy }),
                _ => db.Variables.AsNoTracking().Where(item => item.Id == id && item.ProjectId == project).Select(item => new { item.Revision, item.UpdatedAt, item.UpdatedBy }),
            }).SingleOrDefaultAsync(ct);
            var by = row?.UpdatedBy is { } userId
                ? await db.Users.AsNoTracking().Where(item => item.Id == userId).Select(item => item.UserName).SingleOrDefaultAsync(ct)
                : null;
            conflicts.Add(new { Kind = kind, Id = id, Revision = row?.Revision, UpdatedAt = row?.UpdatedAt, UpdatedBy = by });
        }
        return Results.Conflict(new { error = "Someone else changed these entities. Reload them and decide which version to keep.", conflicts });
    }
}

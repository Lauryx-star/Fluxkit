using System.Text.Json;
using System.Text.RegularExpressions;
using Microsoft.EntityFrameworkCore;

namespace Fluxkit.Server;

public sealed record ChangeOperation(string Op, string Kind, Guid Id, long? ExpectedRevision, JsonElement? Data);
public sealed record ChangeBatch(List<ChangeOperation>? Operations);

public sealed record CollectionInput(string Name, JsonElement? Auth, JsonElement? OpenApi, JsonElement? Postman, string Position);
public sealed record FolderInput(Guid CollectionId, Guid? ParentId, string Name, JsonElement? Auth, string Position);
public sealed record RequestInput(Guid CollectionId, Guid? FolderId, string Name, string Method, string Url, JsonElement Details, string Position);
public sealed record EnvironmentInput(string Name, string Position);
public sealed record VariableInput(string Name, bool Secret, string Position);
public sealed record ProjectInput(string Name, JsonElement? Auth);

/// One entity whose expected revision did not match: it changed or disappeared meanwhile.
public sealed record ChangeConflict(string Kind, Guid Id, long? Revision);

public sealed class ChangeOutcome
{
    public Dictionary<Guid, long> Revisions { get; } = [];
    public List<ChangeConflict> Conflicts { get; } = [];
    public string? Error { get; set; }
}

/// Applies a batch of entity operations to one project inside the caller's transaction.
/// Every operation names the revision it expects; any mismatch makes the whole batch a
/// conflict. Tree rules (parents in the same collection, no folder cycles) are checked on the
/// final state of the batch. Each accepted operation appends a change-feed row.
public static partial class ChangeEngine
{
    public const int MaxOperations = 20_000;
    public const int MaxCollections = 1_000;
    public const int MaxItemsPerCollection = 10_000;
    public const int MaxEnvironments = 100;
    public const int MaxVariables = 1_000;
    public const int MaxDetailsBytes = 4 * 1024 * 1024;
    public const int MaxOpenApiBytes = 4 * 1024 * 1024;
    public const int MaxSmallJsonBytes = 64 * 1024;
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);

    [GeneratedRegex("^[0-9a-z]{1,128}$")]
    private static partial Regex PositionPattern();
    [GeneratedRegex("^[A-Za-z_][A-Za-z0-9_.-]{0,199}$")]
    private static partial Regex VariablePattern();

    public static async Task<ChangeOutcome> ApplyAsync(
        FluxkitDb db, ProjectRecord project, Guid user, IReadOnlyList<ChangeOperation> operations, CancellationToken ct)
    {
        var outcome = new ChangeOutcome();
        if (operations.Count > MaxOperations)
        {
            outcome.Error = "A batch may contain at most 20,000 operations.";
            return outcome;
        }
        var ids = operations.Select(operation => operation.Id).ToHashSet();
        var collections = await db.Collections.Where(item => item.ProjectId == project.Id && ids.Contains(item.Id)).ToDictionaryAsync(item => item.Id, ct);
        var requests = await db.Requests.Where(item => item.ProjectId == project.Id && ids.Contains(item.Id)).ToDictionaryAsync(item => item.Id, ct);
        var environments = await db.Environments.Where(item => item.ProjectId == project.Id && ids.Contains(item.Id)).ToDictionaryAsync(item => item.Id, ct);
        var variables = await db.Variables.Where(item => item.ProjectId == project.Id && ids.Contains(item.Id)).ToDictionaryAsync(item => item.Id, ct);
        // Folders are loaded per affected collection: tree rules need every folder there.
        var touchedCollections = await AffectedCollections(db, project.Id, operations, ct);
        var folders = await db.Folders.Where(item => item.ProjectId == project.Id && touchedCollections.Contains(item.CollectionId)).ToDictionaryAsync(item => item.Id, ct);
        foreach (var folder in await db.Folders.Where(item => item.ProjectId == project.Id && ids.Contains(item.Id) && !touchedCollections.Contains(item.CollectionId)).ToListAsync(ct))
        {
            folders[folder.Id] = folder;
        }
        var knownCollections = (await db.Collections.Where(item => item.ProjectId == project.Id).Select(item => item.Id).ToListAsync(ct)).ToHashSet();
        var deletedCollections = new HashSet<Guid>();
        var deletedFolders = new HashSet<Guid>();
        var deletedRequests = new HashSet<Guid>();
        var deletedEnvironments = new List<Guid>();
        var now = DateTimeOffset.UtcNow;
        var changes = new List<ProjectChangeRecord>();

        foreach (var operation in operations)
        {
            var put = operation.Op == "put";
            if (!put && operation.Op != "delete")
            {
                outcome.Error = "Operations must be put or delete.";
                return outcome;
            }
            if (operation.Id == Guid.Empty)
            {
                outcome.Error = "Every operation needs an entity identifier.";
                return outcome;
            }
            string? error;
            switch (operation.Kind)
            {
                case "project":
                    if (!put || operation.Id != project.Id)
                    {
                        outcome.Error = "Only the project itself can be changed, not deleted, through a batch.";
                        return outcome;
                    }
                    if (operation.ExpectedRevision != project.Revision)
                    {
                        outcome.Conflicts.Add(new("project", project.Id, project.Revision));
                        continue;
                    }
                    var projectInput = Read<ProjectInput>(operation, out error);
                    if (projectInput is null || (error = Name(projectInput.Name)) is not null
                        || (error = SmallJson(projectInput.Auth, "Project authentication")) is not null)
                    {
                        outcome.Error = error ?? "Invalid project data.";
                        return outcome;
                    }
                    project.Name = projectInput.Name.Trim();
                    project.Auth = Raw(projectInput.Auth);
                    Touch(project, user, now);
                    Record(changes, project.Id, "project", project.Id, "put", project.Revision, user, now);
                    outcome.Revisions[project.Id] = project.Revision;
                    continue;
                case "collection":
                    error = Step(operation, collections, outcome, project.Id, user, now, changes, entity =>
                    {
                        var input = Read<CollectionInput>(operation, out var invalid);
                        if (input is null) { return invalid; }
                        if ((invalid = Name(input.Name) ?? Position(input.Position)
                            ?? SmallJson(input.Auth, "Collection authentication") ?? SmallJson(input.Postman, "Postman metadata")
                            ?? SizedJson(input.OpenApi, MaxOpenApiBytes, "OpenAPI metadata")) is not null) { return invalid; }
                        entity.Name = input.Name.Trim();
                        entity.Auth = Raw(input.Auth);
                        entity.OpenApi = Raw(input.OpenApi);
                        entity.Postman = Raw(input.Postman);
                        entity.Position = input.Position;
                        knownCollections.Add(entity.Id);
                        return null;
                    }, db.Collections, deleted => { deletedCollections.Add(deleted.Id); });
                    break;
                case "folder":
                    error = Step(operation, folders, outcome, project.Id, user, now, changes, entity =>
                    {
                        var input = Read<FolderInput>(operation, out var invalid);
                        if (input is null) { return invalid; }
                        if ((invalid = Name(input.Name) ?? Position(input.Position) ?? SmallJson(input.Auth, "Folder authentication")) is not null) { return invalid; }
                        entity.CollectionId = input.CollectionId;
                        entity.ParentId = input.ParentId;
                        entity.Name = input.Name.Trim();
                        entity.Auth = Raw(input.Auth);
                        entity.Position = input.Position;
                        return null;
                    }, db.Folders, deleted => { deletedFolders.Add(deleted.Id); });
                    break;
                case "request":
                    error = Step(operation, requests, outcome, project.Id, user, now, changes, entity =>
                    {
                        var input = Read<RequestInput>(operation, out var invalid);
                        if (input is null) { return invalid; }
                        if ((invalid = Name(input.Name) ?? Position(input.Position)) is not null) { return invalid; }
                        if (string.IsNullOrWhiteSpace(input.Method) || input.Method.Length > 32 || input.Url is null || input.Url.Length > 65_536)
                        {
                            return "Requests need a method of at most 32 characters and a URL of at most 64 KiB.";
                        }
                        if (input.Details.ValueKind != JsonValueKind.Object || input.Details.GetRawText().Length > MaxDetailsBytes)
                        {
                            return "Request details must be an object of at most 4 MiB.";
                        }
                        entity.CollectionId = input.CollectionId;
                        entity.FolderId = input.FolderId;
                        entity.Name = input.Name.Trim();
                        entity.Method = input.Method;
                        entity.Url = input.Url;
                        entity.Details = input.Details.GetRawText();
                        entity.Position = input.Position;
                        return null;
                    }, db.Requests, deleted => { deletedRequests.Add(deleted.Id); });
                    break;
                case "environment":
                    error = Step(operation, environments, outcome, project.Id, user, now, changes, entity =>
                    {
                        var input = Read<EnvironmentInput>(operation, out var invalid);
                        if (input is null) { return invalid; }
                        if ((invalid = Name(input.Name) ?? Position(input.Position)) is not null) { return invalid; }
                        entity.Name = input.Name.Trim();
                        entity.Position = input.Position;
                        return null;
                    }, db.Environments, deleted => deletedEnvironments.Add(deleted.Id));
                    break;
                case "variable":
                    error = Step(operation, variables, outcome, project.Id, user, now, changes, entity =>
                    {
                        var input = Read<VariableInput>(operation, out var invalid);
                        if (input is null) { return invalid; }
                        if (input.Name is null || !VariablePattern().IsMatch(input.Name))
                        {
                            return "Variable names use letters, digits, underscores, dots and hyphens and start with a letter or underscore.";
                        }
                        if ((invalid = Position(input.Position)) is not null) { return invalid; }
                        entity.Name = input.Name;
                        entity.Secret = input.Secret;
                        entity.Position = input.Position;
                        return null;
                    }, db.Variables, _ => { });
                    break;
                default:
                    outcome.Error = "Unknown entity kind.";
                    return outcome;
            }
            if (error is not null)
            {
                outcome.Error = error;
                return outcome;
            }
        }
        if (outcome.Conflicts.Count > 0)
        {
            return outcome;
        }
        outcome.Error = CheckTree(folders, requests, knownCollections, deletedCollections, deletedFolders, deletedRequests);
        if (outcome.Error is not null)
        {
            return outcome;
        }
        // Values of a removed environment have no foreign key (the default slot is `default`).
        foreach (var environment in deletedEnvironments.Select(id => id.ToString()))
        {
            await db.SharedValues.Where(value => value.ProjectId == project.Id && value.EnvironmentId == environment).ExecuteDeleteAsync(ct);
            await db.VaultValues.Where(value => value.ProjectId == project.Id && value.EnvironmentId == environment).ExecuteDeleteAsync(ct);
        }
        // A variable that became secret must not keep shared plaintext, and vice versa.
        foreach (var variable in variables.Values.Where(item => db.Entry(item).State == EntityState.Modified
            && db.Entry(item).Property(entity => entity.Secret).IsModified))
        {
            if (variable.Secret)
            {
                await db.SharedValues.Where(value => value.VariableId == variable.Id).ExecuteDeleteAsync(ct);
            }
            else
            {
                await db.VaultValues.Where(value => value.VariableId == variable.Id).ExecuteDeleteAsync(ct);
            }
        }
        db.ProjectChanges.AddRange(changes);
        return outcome;
    }

    /// After SaveChanges: per-project limits, checked inside the same transaction.
    public static async Task<string?> CheckLimitsAsync(FluxkitDb db, Guid project, CancellationToken ct)
    {
        if (await db.Collections.CountAsync(item => item.ProjectId == project, ct) > MaxCollections)
        {
            return "A project can contain at most 1,000 collections.";
        }
        if (await db.Environments.CountAsync(item => item.ProjectId == project, ct) > MaxEnvironments
            || await db.Variables.CountAsync(item => item.ProjectId == project, ct) > MaxVariables)
        {
            return "A project supports up to 100 environments and 1,000 variables.";
        }
        var folders = await db.Folders.Where(item => item.ProjectId == project).GroupBy(item => item.CollectionId).Select(group => new { group.Key, Count = group.Count() }).ToListAsync(ct);
        var requests = await db.Requests.Where(item => item.ProjectId == project).GroupBy(item => item.CollectionId).Select(group => new { group.Key, Count = group.Count() }).ToListAsync(ct);
        var items = folders.Concat(requests).GroupBy(item => item.Key).Select(group => group.Sum(item => item.Count));
        return items.Any(count => count > MaxItemsPerCollection) ? "A collection can contain at most 10,000 folders and requests." : null;
    }

    private static async Task<HashSet<Guid>> AffectedCollections(FluxkitDb db, Guid project, IReadOnlyList<ChangeOperation> operations, CancellationToken ct)
    {
        var affected = new HashSet<Guid>();
        foreach (var operation in operations.Where(operation => operation.Data is { ValueKind: JsonValueKind.Object } data
            && data.TryGetProperty("collectionId", out _)))
        {
            if (operation.Data!.Value.GetProperty("collectionId").TryGetGuid(out var collection))
            {
                affected.Add(collection);
            }
        }
        var ids = operations.Where(operation => operation.Kind is "folder" or "request").Select(operation => operation.Id).ToHashSet();
        affected.UnionWith(await db.Folders.Where(item => item.ProjectId == project && ids.Contains(item.Id)).Select(item => item.CollectionId).ToListAsync(ct));
        affected.UnionWith(await db.Requests.Where(item => item.ProjectId == project && ids.Contains(item.Id)).Select(item => item.CollectionId).ToListAsync(ct));
        return affected;
    }

    private static string? Step<T>(
        ChangeOperation operation, Dictionary<Guid, T> loaded, ChangeOutcome outcome, Guid project, Guid user,
        DateTimeOffset now, List<ProjectChangeRecord> changes, Func<T, string?> fill, DbSet<T> set, Action<T> deleted)
        where T : ProjectEntity, new()
    {
        loaded.TryGetValue(operation.Id, out var entity);
        var kind = operation.Kind;
        if (operation.Op == "delete")
        {
            if (entity is null || operation.ExpectedRevision != entity.Revision)
            {
                outcome.Conflicts.Add(new(kind, operation.Id, entity?.Revision));
                return null;
            }
            set.Remove(entity);
            deleted(entity);
            Record(changes, project, kind, entity.Id, "delete", entity.Revision, user, now);
            return null;
        }
        if (operation.ExpectedRevision is null)
        {
            if (entity is not null)
            {
                outcome.Conflicts.Add(new(kind, operation.Id, entity.Revision));
                return null;
            }
            entity = new T { Id = operation.Id, ProjectId = project, Revision = 0 };
            var invalid = fill(entity);
            if (invalid is not null) { return invalid; }
            entity.Revision = 1;
            entity.UpdatedAt = now;
            entity.UpdatedBy = user;
            set.Add(entity);
            loaded[entity.Id] = entity;
        }
        else
        {
            if (entity is null || operation.ExpectedRevision != entity.Revision)
            {
                outcome.Conflicts.Add(new(kind, operation.Id, entity?.Revision));
                return null;
            }
            var invalid = fill(entity);
            if (invalid is not null) { return invalid; }
            entity.Revision++;
            entity.UpdatedAt = now;
            entity.UpdatedBy = user;
        }
        Record(changes, project, kind, entity.Id, "put", entity.Revision, user, now);
        outcome.Revisions[entity.Id] = entity.Revision;
        return null;
    }

    /// Folders and requests of the final state must point at live parents in their own
    /// collection, and folder parent chains must not loop.
    private static string? CheckTree(
        Dictionary<Guid, FolderRecord> folders, Dictionary<Guid, RequestRecord> requests,
        HashSet<Guid> knownCollections, HashSet<Guid> deletedCollections, HashSet<Guid> deletedFolders,
        HashSet<Guid> deletedRequests)
    {
        bool Gone(FolderRecord folder)
        {
            var current = folder;
            for (var depth = 0; depth < 64; depth++)
            {
                if (deletedFolders.Contains(current.Id) || deletedCollections.Contains(current.CollectionId)) { return true; }
                if (current.ParentId is not { } parent || !folders.TryGetValue(parent, out var next)) { return false; }
                current = next;
            }
            return false;
        }
        foreach (var folder in folders.Values.Where(folder => !Gone(folder)))
        {
            if (!knownCollections.Contains(folder.CollectionId) || deletedCollections.Contains(folder.CollectionId))
            {
                return "A folder references a collection that does not exist.";
            }
            var seen = new HashSet<Guid> { folder.Id };
            var current = folder;
            while (current.ParentId is { } parent)
            {
                if (!folders.TryGetValue(parent, out var next) || Gone(next) || next.CollectionId != folder.CollectionId)
                {
                    return "A folder's parent must be a folder in the same collection.";
                }
                if (!seen.Add(next.Id) || seen.Count > 64)
                {
                    return "Folder parents must not form a cycle or nest deeper than 64 levels.";
                }
                current = next;
            }
        }
        foreach (var request in requests.Values.Where(request => !deletedRequests.Contains(request.Id)))
        {
            if (deletedCollections.Contains(request.CollectionId)) { continue; }
            if (request.FolderId is { } folderId && folders.TryGetValue(folderId, out var parent) && Gone(parent)) { continue; }
            if (!knownCollections.Contains(request.CollectionId))
            {
                return "A request references a collection that does not exist.";
            }
            if (request.FolderId is { } id && (!folders.TryGetValue(id, out var folder) || folder.CollectionId != request.CollectionId))
            {
                return "A request's folder must be a folder in the same collection.";
            }
        }
        return null;
    }

    private static T? Read<T>(ChangeOperation operation, out string? error) where T : class
    {
        error = "Every put needs valid data for its kind.";
        if (operation.Data is not { ValueKind: JsonValueKind.Object } data) { return null; }
        try
        {
            var value = data.Deserialize<T>(Json);
            if (value is not null) { error = null; }
            return value;
        }
        catch (JsonException) { return null; }
    }

    private static string? Name(string? name) =>
        name is null || name.Trim().Length is < 1 or > 200 || name.Any(char.IsControl)
            ? "Names must contain 1 to 200 characters without control characters." : null;

    private static string? Position(string? position) =>
        position is null || !PositionPattern().IsMatch(position) ? "Positions are rank strings of 1 to 128 characters (0-9, a-z)." : null;

    /// Authentication and Postman metadata: the desktop model's JSON (an object, or a string
    /// such as `"none"` for unit variants), kept opaque but size-bounded.
    private static string? SmallJson(JsonElement? value, string label) =>
        value is { ValueKind: not JsonValueKind.Null and not JsonValueKind.Undefined } element
            && (element.ValueKind is not (JsonValueKind.Object or JsonValueKind.String) || element.GetRawText().Length > MaxSmallJsonBytes)
            ? $"{label} must be a JSON object or string within the size limit." : null;

    private static string? SizedJson(JsonElement? value, int limit, string label) =>
        value is { ValueKind: not JsonValueKind.Null and not JsonValueKind.Undefined } element
            && (element.ValueKind != JsonValueKind.Object || element.GetRawText().Length > limit)
            ? $"{label} must be an object within the size limit." : null;

    public static string? Raw(JsonElement? value) =>
        value is { ValueKind: not JsonValueKind.Null and not JsonValueKind.Undefined } element ? element.GetRawText() : null;

    private static void Touch(ProjectRecord project, Guid user, DateTimeOffset now)
    {
        project.Revision++;
        project.UpdatedAt = now;
        project.UpdatedBy = user;
    }

    private static void Record(List<ProjectChangeRecord> changes, Guid project, string kind, Guid id, string operation, long revision, Guid user, DateTimeOffset now) =>
        changes.Add(new ProjectChangeRecord { ProjectId = project, EntityKind = kind, EntityId = id, Operation = operation, Revision = revision, ChangedBy = user, ChangedAt = now });
}

using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using System.Text.Json;
using Microsoft.EntityFrameworkCore;

namespace Fluxkit.Server;

public sealed record ValueSlot(Guid VariableId, string EnvironmentId);
public sealed record SharedValue(Guid VariableId, string EnvironmentId, string Value);
public sealed record CipherValue(Guid VariableId, string EnvironmentId, string Nonce, string Ciphertext);
public sealed record KdfParameters(string Algorithm, int Version, int MemoryKiB, int Iterations, int Parallelism);
public sealed record VaultProfile(int SchemaVersion, string Algorithm, KdfParameters Kdf, string Salt, string Nonce, string WrappedKey);
public sealed record VaultInput(VaultProfile Profile);
public sealed record ValueUpdate<T>(long ProjectRevision, List<T> Values);

/// Shared data and per-user ciphertext never enter the portable definition snapshot.
public static class ValueEndpoints
{
    private static readonly JsonSerializerOptions Json = new(JsonSerializerDefaults.Web);
    public static string Encode<T>(T value) => JsonSerializer.Serialize(value, Json);
    private static Guid User(ClaimsPrincipal caller) => Guid.Parse(caller.FindFirstValue(JwtRegisteredClaimNames.Sub)!);
    private static Task<ProjectRecord?> Project(FluxkitDb db, Guid id, Guid user, CancellationToken ct) =>
        db.Projects.AsNoTracking().SingleOrDefaultAsync(project => project.Id == id &&
            (project.Kind == "personal" && project.OwnerId == user || project.Kind == "team" &&
            db.Memberships.Any(member => member.ProjectId == id && member.UserId == user)), ct);

    public static void Map(WebApplication app)
    {
        app.MapGet("/api/account/vault", async (ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var id = User(caller);
            var vault = await db.AccountVaults.AsNoTracking().SingleOrDefaultAsync(vault => vault.UserId == id, ct);
            return Results.Ok(new { revision = vault?.Revision ?? 0, profile = vault is null ? null : JsonSerializer.Deserialize<VaultProfile>(vault.Profile, Json) });
        }).RequireAuthorization();
        app.MapPut("/api/account/vault", async (VaultInput input, ClaimsPrincipal caller, HttpRequest request, FluxkitDb db, CancellationToken ct) =>
        {
            if (!Expected(request, out var expected)) { return Results.StatusCode(428); }
            if (input is null || !ValidProfile(input.Profile)) { return Results.BadRequest(); }
            var id = User(caller);
            var vault = await db.AccountVaults.SingleOrDefaultAsync(vault => vault.UserId == id, ct);
            if ((vault?.Revision ?? 0) != expected) { return Results.Conflict(); }
            if (vault is null)
            {
                vault = new AccountVault { UserId = id };
                db.AccountVaults.Add(vault);
            }
            else { vault.Revision++; }
            vault.Profile = Encode(input.Profile);
            try { await db.SaveChangesAsync(ct); }
            catch (DbUpdateException) { return Results.Conflict(); }
            return Results.Ok(new { vault.Revision, input.Profile });
        }).RequireAuthorization();

        app.MapGet("/api/projects/{id:guid}/values", async (Guid id, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var project = await Project(db, id, User(caller), ct);
            if (project is null) { return Results.NotFound(); }
            var values = JsonSerializer.Deserialize<List<SharedValue>>(project.SharedValues, Json) ?? [];
            var policy = DefinitionPolicy.Read(project.Snapshot);
            return Results.Ok(new { revision = project.SharedValuesRevision, projectRevision = project.Revision,
                values = values.Where(value => policy.Allows(value.VariableId, value.EnvironmentId, false)) });
        }).RequireAuthorization();
        app.MapPut("/api/projects/{id:guid}/values", async (Guid id, ValueUpdate<SharedValue> input, ClaimsPrincipal caller, HttpRequest request, FluxkitDb db, CancellationToken ct) =>
        {
            var project = await Project(db, id, User(caller), ct);
            if (project is null) { return Results.NotFound(); }
            if (!Expected(request, out var expected)) { return Results.StatusCode(428); }
            if (input is null || !ValidShared(project.Snapshot, input.Values)) { return Results.BadRequest(); }
            var json = Encode(input.Values);
            var changed = await db.Projects.Where(project => project.Id == id && project.Revision == input.ProjectRevision && project.SharedValuesRevision == expected)
                .ExecuteUpdateAsync(update => update.SetProperty(project => project.SharedValues, json)
                    .SetProperty(project => project.SharedValuesRevision, expected + 1), ct);
            if (changed == 0) { return Results.Conflict(); }
            return Results.Ok(new { revision = expected + 1, projectRevision = input.ProjectRevision, input.Values });
        }).RequireAuthorization();
        app.MapGet("/api/projects/{id:guid}/vault-values", async (Guid id, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            var project = await Project(db, id, user, ct);
            if (project is null) { return Results.NotFound(); }
            var row = await db.ProjectSecrets.AsNoTracking().SingleOrDefaultAsync(row => row.ProjectId == id && row.UserId == user, ct);
            var values = row is null ? [] : JsonSerializer.Deserialize<List<CipherValue>>(row.Values, Json) ?? [];
            var policy = DefinitionPolicy.Read(project.Snapshot);
            return Results.Ok(new { revision = row?.Revision ?? 0, projectRevision = project.Revision,
                values = values.Where(value => policy.Allows(value.VariableId, value.EnvironmentId, true)) });
        }).RequireAuthorization();
        app.MapPut("/api/projects/{id:guid}/vault-values", async (Guid id, ValueUpdate<CipherValue> input, ClaimsPrincipal caller, HttpRequest request, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            if (await Project(db, id, user, ct) is null) { return Results.NotFound(); }
            if (!Expected(request, out var expected)) { return Results.StatusCode(428); }
            // Lock the definition row until ciphertext commits: classification cannot change
            // between validation and persistence. Snapshot UPDATE waits for this same row lock.
            await using var transaction = await db.Database.BeginTransactionAsync(ct);
            var project = (await db.Projects.FromSqlInterpolated($"SELECT * FROM \"Projects\" WHERE \"Id\" = {id} FOR UPDATE")
                .AsNoTracking().ToListAsync(ct)).Single();
            if (input is null || project.Revision != input.ProjectRevision) { return Results.Conflict(); }
            if (!ValidSecrets(project.Snapshot, input.Values) || !await db.AccountVaults.AnyAsync(vault => vault.UserId == user, ct)) { return Results.BadRequest(); }
            var row = await db.ProjectSecrets.SingleOrDefaultAsync(row => row.ProjectId == id && row.UserId == user, ct);
            if ((row?.Revision ?? 0) != expected) { return Results.Conflict(); }
            if (row is null)
            {
                row = new ProjectSecrets { ProjectId = id, UserId = user };
                db.ProjectSecrets.Add(row);
            }
            else { row.Revision++; }
            row.Values = Encode(input.Values);
            await db.SaveChangesAsync(ct);
            await transaction.CommitAsync(ct);
            return Results.Ok(new { row.Revision, projectRevision = project.Revision, input.Values });
        }).RequireAuthorization();
    }

    private static bool Expected(HttpRequest request, out long revision) =>
        long.TryParse(request.Headers.IfMatch.ToString().Trim('"'), out revision) && revision >= 0;
    public static bool ValidShared(string snapshot, List<SharedValue>? values)
    {
        if (values is null || values.Count > 1000) { return false; }
        var policy = DefinitionPolicy.Read(snapshot);
        return values.All(value => value is not null && value.Value is not null && Encoding.UTF8.GetByteCount(value.Value) <= 2560
            && policy.Allows(value.VariableId, value.EnvironmentId, false))
            && values.Select(value => new ValueSlot(value.VariableId, value.EnvironmentId)).Distinct().Count() == values.Count;
    }
    public static bool ValidSecrets(string snapshot, List<CipherValue>? values)
    {
        if (values is null || values.Count > 1000) { return false; }
        var policy = DefinitionPolicy.Read(snapshot);
        return values.All(value => value is not null && policy.Allows(value.VariableId, value.EnvironmentId, true)
            && Base64(value.Nonce, 12, 12) && Base64(value.Ciphertext, 16, 2576))
            && values.Select(value => new ValueSlot(value.VariableId, value.EnvironmentId)).Distinct().Count() == values.Count;
    }
    public static bool ValidProfile(VaultProfile? value) => value is not null && value.SchemaVersion == 1 && value.Algorithm == "AES-256-GCM"
        && value.Kdf is { Algorithm: "Argon2id", Version: 19, MemoryKiB: >= 65536 and <= 262144, Iterations: >= 3 and <= 10, Parallelism: >= 1 and <= 4 }
        && Base64(value.Salt, 16, 16) && Base64(value.Nonce, 12, 12) && Base64(value.WrappedKey, 48, 48);
    private static bool Base64(string? value, int min, int max)
    {
        if (value is null || value.Length > (max + 2) / 3 * 4) { return false; }
        try { var length = Convert.FromBase64String(value).Length; return length >= min && length <= max; }
        catch (FormatException) { return false; }
    }
    private sealed record DefinitionPolicy(Dictionary<Guid, bool> Variables, HashSet<string> Environments)
    {
        public bool Allows(Guid variable, string? environment, bool secret) => variable != Guid.Empty
            && Variables.TryGetValue(variable, out var classification) && classification == secret
            && environment is not null && Environments.Contains(environment);
        public static DefinitionPolicy Read(string snapshot)
        {
            try
            {
                using var document = JsonDocument.Parse(snapshot);
                var definitions = document.RootElement.GetProperty("environments");
                var variables = definitions.GetProperty("variables").EnumerateArray().ToDictionary(item => item.GetProperty("id").GetGuid(),
                    item => !item.TryGetProperty("secret", out var classification) || classification.GetBoolean());
                var environments = definitions.GetProperty("environments").EnumerateArray().Select(item => item.GetProperty("id").GetGuid().ToString()).ToHashSet();
                environments.Add("default");
                return new(variables, environments);
            }
            catch (Exception error) when (error is JsonException or InvalidOperationException or FormatException or KeyNotFoundException or ArgumentException)
            { return new([], []); }
        }
    }
}

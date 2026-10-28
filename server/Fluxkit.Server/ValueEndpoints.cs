using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using System.Text.Json;
using Microsoft.EntityFrameworkCore;

namespace Fluxkit.Server;

public sealed record KdfParameters(string Algorithm, int Version, int MemoryKiB, int Iterations, int Parallelism);
public sealed record VaultProfile(int SchemaVersion, string Algorithm, KdfParameters Kdf, string Salt, string Nonce, string WrappedKey);
public sealed record VaultInput(VaultProfile Profile);
public sealed record SharedSlotInput(Guid VariableId, string EnvironmentId, string Value, long? ExpectedRevision = null);
public sealed record VaultSlotInput(Guid VariableId, string EnvironmentId, string Nonce, string Ciphertext, long? ExpectedRevision = null);
public sealed record SharedValueInput(string Value);
public sealed record VaultValueInput(string Nonce, string Ciphertext);
public sealed record ValueBatch(List<SharedSlotInput>? Shared, List<VaultSlotInput>? Vault);

/// Values live in one row per slot (variable + environment) with their own revision, so two
/// people setting different variables never conflict. Shared rows hold ordinary plaintext for
/// all members; vault rows hold one user's client-encrypted secret and are only ever returned
/// to that user.
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
        app.MapGet("/api/projects/{id:guid}/values", async (Guid id, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            if (await Project(db, id, User(caller), ct) is null) { return Results.NotFound(); }
            var values = await db.SharedValues.AsNoTracking().Where(value => value.ProjectId == id)
                .OrderBy(value => value.VariableId).ThenBy(value => value.EnvironmentId)
                .Select(value => new { value.VariableId, value.EnvironmentId, value.Value, value.Revision }).ToListAsync(ct);
            return Results.Ok(new { values });
        }).RequireAuthorization();
        app.MapGet("/api/projects/{id:guid}/vault-values", async (Guid id, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            if (await Project(db, id, user, ct) is null) { return Results.NotFound(); }
            var values = await db.VaultValues.AsNoTracking().Where(value => value.ProjectId == id && value.UserId == user)
                .OrderBy(value => value.VariableId).ThenBy(value => value.EnvironmentId)
                .Select(value => new { value.VariableId, value.EnvironmentId, value.Nonce, value.Ciphertext, value.Revision }).ToListAsync(ct);
            return Results.Ok(new { values });
        }).RequireAuthorization();
        app.MapPut("/api/projects/{id:guid}/values/{variableId:guid}/{environmentId}", async (Guid id, Guid variableId, string environmentId,
            SharedValueInput input, HttpRequest request, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            if (await Editable(db, id, user, ct) is not true) { return Results.NotFound(); }
            if (!Expected(request, out var expected)) { return Results.StatusCode(428); }
            if (input?.Value is null) { return Results.BadRequest(); }
            return await ApplySlotsAsync(db, id, user, [new(variableId, environmentId, input.Value, expected)], [], ct)
                ?? Results.Ok(await SlotRevision(db, variableId, environmentId, null, ct));
        }).RequireAuthorization();
        app.MapPut("/api/projects/{id:guid}/vault-values/{variableId:guid}/{environmentId}", async (Guid id, Guid variableId, string environmentId,
            VaultValueInput input, HttpRequest request, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            if (await Project(db, id, user, ct) is null) { return Results.NotFound(); }
            if (!Expected(request, out var expected)) { return Results.StatusCode(428); }
            if (input?.Nonce is null || input.Ciphertext is null) { return Results.BadRequest(); }
            return await ApplySlotsAsync(db, id, user, [], [new(variableId, environmentId, input.Nonce, input.Ciphertext, expected)], ct)
                ?? Results.Ok(await SlotRevision(db, variableId, environmentId, user, ct));
        }).RequireAuthorization();
        app.MapDelete("/api/projects/{id:guid}/values/{variableId:guid}/{environmentId}", async (Guid id, Guid variableId, string environmentId,
            HttpRequest request, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            if (await Editable(db, id, User(caller), ct) is not true) { return Results.NotFound(); }
            if (!Expected(request, out var expected)) { return Results.StatusCode(428); }
            var deleted = await db.SharedValues.Where(value => value.ProjectId == id && value.VariableId == variableId
                && value.EnvironmentId == environmentId && value.Revision == expected).ExecuteDeleteAsync(ct);
            return deleted == 1 ? Results.NoContent() : Results.Conflict(new { error = "This value changed or was removed. Reload values." });
        }).RequireAuthorization();
        app.MapDelete("/api/projects/{id:guid}/vault-values/{variableId:guid}/{environmentId}", async (Guid id, Guid variableId, string environmentId,
            HttpRequest request, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            if (await Project(db, id, user, ct) is null) { return Results.NotFound(); }
            if (!Expected(request, out var expected)) { return Results.StatusCode(428); }
            var deleted = await db.VaultValues.Where(value => value.ProjectId == id && value.UserId == user && value.VariableId == variableId
                && value.EnvironmentId == environmentId && value.Revision == expected).ExecuteDeleteAsync(ct);
            return deleted == 1 ? Results.NoContent() : Results.Conflict(new { error = "This value changed or was removed. Reload values." });
        }).RequireAuthorization();
        app.MapPost("/api/projects/{id:guid}/values/batch", async (Guid id, ValueBatch input, ClaimsPrincipal caller, FluxkitDb db, CancellationToken ct) =>
        {
            var user = User(caller);
            if (await Project(db, id, user, ct) is null) { return Results.NotFound(); }
            var shared = input?.Shared ?? [];
            if (shared.Count > 0 && await Editable(db, id, user, ct) is not true) { return Results.Forbid(); }
            await using var transaction = await db.Database.BeginTransactionAsync(ct);
            var failed = await ApplySlotsAsync(db, id, user, shared, input?.Vault ?? [], ct);
            if (failed is not null) { return failed; }
            await transaction.CommitAsync(ct);
            return Results.NoContent();
        }).RequireAuthorization();

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

    }

    private static async Task<bool?> Editable(FluxkitDb db, Guid id, Guid user, CancellationToken ct)
    {
        var project = await db.Projects.AsNoTracking().SingleOrDefaultAsync(item => item.Id == id, ct);
        if (project is null || await Project(db, id, user, ct) is null) { return null; }
        return await ProjectEndpoints.Writable(db, project, user, ct);
    }

    private static async Task<object> SlotRevision(FluxkitDb db, Guid variable, string environment, Guid? user, CancellationToken ct) =>
        new
        {
            Revision = user is { } owner
                ? await db.VaultValues.Where(value => value.UserId == owner && value.VariableId == variable && value.EnvironmentId == environment).Select(value => value.Revision).SingleAsync(ct)
                : await db.SharedValues.Where(value => value.VariableId == variable && value.EnvironmentId == environment).Select(value => value.Revision).SingleAsync(ct)
        };

    /// Writes value slots of one project. `ExpectedRevision` null or 0 creates a slot that must
    /// not exist yet; any other number must match the stored revision. Returns null on success
    /// (changes saved, caller commits) or the error/conflict result.
    public static async Task<IResult?> ApplySlotsAsync(FluxkitDb db, Guid project, Guid user,
        IReadOnlyList<SharedSlotInput> shared, IReadOnlyList<VaultSlotInput> vault, CancellationToken ct)
    {
        if (shared.Count + vault.Count > 2000) { return Results.BadRequest(new { error = "Too many values in one request." }); }
        var variables = await db.Variables.AsNoTracking().Where(item => item.ProjectId == project)
            .ToDictionaryAsync(item => item.Id, item => item.Secret, ct);
        var environments = (await db.Environments.AsNoTracking().Where(item => item.ProjectId == project)
            .Select(item => item.Id).ToListAsync(ct)).Select(item => item.ToString()).ToHashSet();
        environments.Add("default");
        bool Valid(Guid variable, string environment, bool secret) =>
            variables.TryGetValue(variable, out var isSecret) && isSecret == secret && environment is not null && environments.Contains(environment);
        if (shared.Select(slot => (slot.VariableId, slot.EnvironmentId)).Distinct().Count() != shared.Count
            || vault.Select(slot => (slot.VariableId, slot.EnvironmentId)).Distinct().Count() != vault.Count)
        {
            return Results.BadRequest(new { error = "Each value slot may appear only once." });
        }
        if (shared.Any(slot => slot.Value is null || Encoding.UTF8.GetByteCount(slot.Value) > 2560 || !Valid(slot.VariableId, slot.EnvironmentId, false)))
        {
            return Results.BadRequest(new { error = "Shared values need an existing non-secret variable, a known environment and at most 2,560 bytes." });
        }
        if (vault.Any(slot => !Valid(slot.VariableId, slot.EnvironmentId, true) || !Base64(slot.Nonce, 12, 12) || !Base64(slot.Ciphertext, 16, 2576)))
        {
            return Results.BadRequest(new { error = "Secret values need an existing secret variable, a known environment and a valid ciphertext." });
        }
        if (vault.Count > 0 && !await db.AccountVaults.AnyAsync(item => item.UserId == user, ct))
        {
            return Results.BadRequest(new { error = "Set up your account vault before storing secrets." });
        }
        var now = DateTimeOffset.UtcNow;
        foreach (var slot in shared)
        {
            var row = await db.SharedValues.SingleOrDefaultAsync(value => value.VariableId == slot.VariableId && value.EnvironmentId == slot.EnvironmentId, ct);
            if ((slot.ExpectedRevision ?? 0) == 0)
            {
                if (row is not null) { return SlotConflict(row.Revision); }
                db.SharedValues.Add(new SharedValueRecord { ProjectId = project, VariableId = slot.VariableId, EnvironmentId = slot.EnvironmentId, Value = slot.Value, UpdatedBy = user, UpdatedAt = now });
            }
            else
            {
                if (row is null || row.Revision != slot.ExpectedRevision) { return SlotConflict(row?.Revision); }
                row.Value = slot.Value; row.Revision++; row.UpdatedAt = now; row.UpdatedBy = user;
            }
        }
        foreach (var slot in vault)
        {
            var row = await db.VaultValues.SingleOrDefaultAsync(value => value.UserId == user && value.VariableId == slot.VariableId && value.EnvironmentId == slot.EnvironmentId, ct);
            if ((slot.ExpectedRevision ?? 0) == 0)
            {
                if (row is not null) { return SlotConflict(row.Revision); }
                db.VaultValues.Add(new VaultValueRecord { ProjectId = project, UserId = user, VariableId = slot.VariableId, EnvironmentId = slot.EnvironmentId, Nonce = slot.Nonce, Ciphertext = slot.Ciphertext, UpdatedAt = now });
            }
            else
            {
                if (row is null || row.Revision != slot.ExpectedRevision) { return SlotConflict(row?.Revision); }
                row.Nonce = slot.Nonce; row.Ciphertext = slot.Ciphertext; row.Revision++; row.UpdatedAt = now;
            }
        }
        try { await db.SaveChangesAsync(ct); }
        catch (DbUpdateException) { return SlotConflict(null); }
        return null;
    }

    private static IResult SlotConflict(long? current) =>
        Results.Conflict(new { error = "This value changed meanwhile. Reload values; your input is still open.", revision = current });

    private static bool Expected(HttpRequest request, out long revision) =>
        long.TryParse(request.Headers.IfMatch.ToString().Trim('"'), out revision) && revision >= 0;
    public static bool ValidProfile(VaultProfile? value) => value is not null && value.SchemaVersion == 1 && value.Algorithm == "AES-256-GCM"
        && value.Kdf is { Algorithm: "Argon2id", Version: 19, MemoryKiB: >= 65536 and <= 262144, Iterations: >= 3 and <= 10, Parallelism: >= 1 and <= 4 }
        && Base64(value.Salt, 16, 16) && Base64(value.Nonce, 12, 12) && Base64(value.WrappedKey, 48, 48);
    private static bool Base64(string? value, int min, int max)
    {
        if (value is null || value.Length > (max + 2) / 3 * 4) { return false; }
        try { var length = Convert.FromBase64String(value).Length; return length >= min && length <= max; }
        catch (FormatException) { return false; }
    }
}

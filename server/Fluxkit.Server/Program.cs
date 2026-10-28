using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using System.Threading.RateLimiting;
using Fluxkit.Server;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.Identity;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;

var builder = WebApplication.CreateBuilder(args);
var mode = builder.Configuration["Auth:Mode"] ?? "local";
mode = mode.ToLowerInvariant();
if (mode is not ("local" or "oidc"))
{
    throw new InvalidOperationException("Auth:Mode must be local or oidc.");
}
if (mode == "oidc")
{
    OidcLogin.Configure(builder);
}

var connection = builder.Configuration.GetConnectionString("Database")
    ?? throw new InvalidOperationException("ConnectionStrings:Database must be configured.");
var databasePassword = new Npgsql.NpgsqlConnectionStringBuilder(connection).Password ?? "";
if (IsExampleSecret(databasePassword))
    throw new InvalidOperationException("Replace the example PostgreSQL password before starting the server.");
var signingSecret = builder.Configuration["Auth:SigningKey"]
    ?? throw new InvalidOperationException("Auth:SigningKey must be configured with at least 32 random bytes.");
if (IsExampleSecret(signingSecret))
    throw new InvalidOperationException("Replace the example signing key before starting the server.");
var signingBytes = Encoding.UTF8.GetBytes(signingSecret);
if (signingBytes.Length < 32)
    throw new InvalidOperationException("Auth:SigningKey must be at least 32 bytes.");
if (mode == "local" && (string.IsNullOrWhiteSpace(builder.Configuration["Bootstrap:AdminUsername"])
    || string.IsNullOrWhiteSpace(builder.Configuration["Bootstrap:AdminPassword"])))
    throw new InvalidOperationException("Bootstrap administrator username and password are required for initial setup.");
if (mode == "local" && (IsExampleSecret(builder.Configuration["Bootstrap:AdminUsername"]!)
    || IsExampleSecret(builder.Configuration["Bootstrap:AdminPassword"]!)))
    throw new InvalidOperationException("Replace the example administrator credentials before starting the server.");
var signingKey = new SymmetricSecurityKey(signingBytes);

builder.Services.AddDbContext<FluxkitDb>(options => options.UseNpgsql(connection));
builder.Services.AddIdentityCore<FluxkitUser>(options =>
    {
        options.User.RequireUniqueEmail = false;
        options.Password.RequiredLength = 12;
        options.Password.RequireDigit = true;
        options.Password.RequireLowercase = true;
        options.Password.RequireUppercase = true;
        options.Password.RequireNonAlphanumeric = true;
        options.Lockout.MaxFailedAccessAttempts = 5;
        options.Lockout.DefaultLockoutTimeSpan = TimeSpan.FromMinutes(15);
        options.Lockout.AllowedForNewUsers = true;
    })
    .AddRoles<IdentityRole<Guid>>()
    .AddEntityFrameworkStores<FluxkitDb>()
    .AddSignInManager();
builder.Services.AddAuthentication(JwtBearerDefaults.AuthenticationScheme)
    .AddJwtBearer(options =>
    {
        options.MapInboundClaims = false;
        options.TokenValidationParameters = new TokenValidationParameters
        {
            ValidateIssuer = true,
            ValidIssuer = builder.Configuration["Auth:Issuer"] ?? "fluxkit-self-hosted",
            ValidateAudience = true,
            ValidAudience = builder.Configuration["Auth:Audience"] ?? "fluxkit-desktop",
            ValidateLifetime = true,
            ValidateIssuerSigningKey = true,
            IssuerSigningKey = signingKey,
            RoleClaimType = "role",
            ClockSkew = TimeSpan.FromSeconds(30)
        };
        options.Events = new JwtBearerEvents
        {
            OnTokenValidated = async context =>
            {
                var subject = context.Principal?.FindFirstValue(JwtRegisteredClaimNames.Sub);
                if (context.Principal?.FindFirstValue("auth_mode") != mode || !Guid.TryParse(subject, out var id))
                {
                    context.Fail("Invalid account token.");
                    return;
                }
                var db = context.HttpContext.RequestServices.GetRequiredService<FluxkitDb>();
                if (!await db.Users.AnyAsync(user => user.Id == id && user.Status == "active"))
                    context.Fail("Account is not active.");
            }
        };
    });
builder.Services.AddAuthorization();
builder.Services.AddProblemDetails();
builder.Services.AddRateLimiter(options => options.AddPolicy("auth", context =>
    RateLimitPartition.GetFixedWindowLimiter(
        context.Connection.RemoteIpAddress?.ToString() ?? "unknown",
        _ => new FixedWindowRateLimiterOptions
        {
            PermitLimit = 10,
            Window = TimeSpan.FromMinutes(1),
            QueueLimit = 0,
            AutoReplenishment = true
        })));

builder.Services.AddRateLimiter(options => options.AddPolicy("oidc-poll", context =>
    RateLimitPartition.GetFixedWindowLimiter(context.Connection.RemoteIpAddress?.ToString() ?? "unknown",
        _ => new FixedWindowRateLimiterOptions { PermitLimit = 120, Window = TimeSpan.FromMinutes(1), QueueLimit = 0 })));
var app = builder.Build();
app.UseExceptionHandler();
app.UseRateLimiter();
app.UseAuthentication();
app.UseAuthorization();
ValueEndpoints.Map(app);
app.MapGet("/health/ready", async (FluxkitDb db, CancellationToken cancellationToken) =>
{
    await db.Database.CanConnectAsync(cancellationToken);
    return Results.Ok(new { status = "ready" });
});

app.MapGet("/api/auth/mode", () => Results.Ok(new { mode })).AllowAnonymous();
if (mode == "local")
{
    app.MapPost("/api/auth/register", async (
        RegisterInput input, UserManager<FluxkitUser> users, CancellationToken cancellationToken) =>
    {
        if (input is null || string.IsNullOrWhiteSpace(input.Username) || input.Password is null)
            return Results.BadRequest();
        var username = input.Username.Trim();
        if (username.Length is < 3 or > 64 || username.Any(char.IsControl)
            || input.Password.Length is < 12 or > 256)
            return Results.ValidationProblem(new Dictionary<string, string[]>
            {
                ["credentials"] = ["Choose a username of 3 to 64 characters and a password of at least 12 characters."]
            });
        var user = new FluxkitUser { Id = Guid.NewGuid(), UserName = username, Status = "pending" };
        var created = await users.CreateAsync(user, input.Password);
        if (!created.Succeeded)
            return Results.Conflict(new { error = "Account could not be created. The username may already be in use." });
        return Results.Accepted(value: new { status = "pendingApproval" });
    }).AllowAnonymous().RequireRateLimiting("auth");

    app.MapPost("/api/auth/login", async (
        LoginInput input, UserManager<FluxkitUser> users, SignInManager<FluxkitUser> signIn,
        IConfiguration configuration) =>
    {
        if (input is null || string.IsNullOrWhiteSpace(input.Username) || input.Password is null)
            return Results.Unauthorized();
        var user = await users.FindByNameAsync(input.Username.Trim());
        if (user is null || user.Status != "active")
            return Results.Unauthorized();
        var result = await signIn.CheckPasswordSignInAsync(user, input.Password, lockoutOnFailure: true);
        if (!result.Succeeded)
            return Results.Unauthorized();
        return Results.Ok(AuthTokens.Create(user, configuration));
    }).AllowAnonymous().RequireRateLimiting("auth");

}
else { OidcLogin.Map(app); }

app.MapGet("/api/account", async (ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var userId = UserId(principal);
    var username = await db.Users.AsNoTracking()
        .Where(user => user.Id == userId)
        .Select(user => user.UserName)
        .SingleAsync(ct);
    return Results.Ok(new
    {
        Id = userId,
        UserName = username ?? "",
        IsServerAdmin = principal.IsInRole("serverAdmin")
    });
}).RequireAuthorization();

app.MapGet("/api/admin/pending-users", async (FluxkitDb db, CancellationToken ct) =>
    Results.Ok(await db.Users.AsNoTracking().Where(user => user.Status == "pending")
        .OrderBy(user => user.UserName).Select(user => new { user.Id, user.UserName, user.CreatedAt })
        .ToListAsync(ct)))
    .RequireAuthorization(policy => policy.RequireRole("serverAdmin"));

app.MapGet("/api/admin/users", async (FluxkitDb db, CancellationToken ct) =>
    Results.Ok(await db.Users.AsNoTracking()
        .OrderBy(user => user.UserName)
        .Select(user => new { user.Id, user.UserName, user.Status, user.IsServerAdmin, user.CreatedAt })
        .ToListAsync(ct)))
    .RequireAuthorization(policy => policy.RequireRole("serverAdmin"));

app.MapPost("/api/admin/users/{id:guid}/approve", async (
    Guid id, FluxkitDb db, CancellationToken ct) =>
{
    var user = await db.Users.SingleOrDefaultAsync(candidate => candidate.Id == id, ct);
    if (user is null) return Results.NotFound();
    if (user.Status != "pending") return Results.Conflict(new { error = "Only pending accounts can be approved." });
    user.Status = "active";
    if (!await db.Projects.AnyAsync(project => project.OwnerId == id && project.Kind == "personal", ct))
        db.Projects.Add(AuthTokens.CreatePersonalProject(id, user.UserName ?? "User"));
    await db.SaveChangesAsync(ct);
    return Results.NoContent();
}).RequireAuthorization(policy => policy.RequireRole("serverAdmin"));

app.MapPut("/api/admin/users/{id:guid}/status", async (
    Guid id, UserStatusInput input, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var user = await db.Users.SingleOrDefaultAsync(candidate => candidate.Id == id, ct);
    if (user is null) return Results.NotFound();
    if (input is null || input.Status is not ("active" or "disabled")) return Results.BadRequest();
    if (user.Id == UserId(principal) && input.Status != "active")
        return Results.Conflict(new { error = "The current server administrator cannot disable their own account." });
    if (user.IsServerAdmin && input.Status != "active"
        && !await db.Users.AnyAsync(candidate => candidate.Id != id && candidate.IsServerAdmin
            && candidate.Status == "active", ct))
        return Results.Conflict(new { error = "At least one active server administrator must remain." });
    if (user.Status == "pending") return Results.Conflict(new { error = "Pending accounts must use the approval action." });
    user.Status = input.Status;
    await db.SaveChangesAsync(ct);
    return Results.NoContent();
}).RequireAuthorization(policy => policy.RequireRole("serverAdmin"));

app.MapGet("/api/projects", async (ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var userId = UserId(principal);
    var projects = await db.Projects.AsNoTracking()
        .Where(project => project.OwnerId == userId
            || db.Memberships.Any(membership => membership.ProjectId == project.Id && membership.UserId == userId))
        .OrderBy(project => project.Name)
        .Select(project => new
        {
            project.Id,
            project.Name,
            project.Kind,
            project.Revision,
            CanEdit = project.Kind == "personal"
                ? project.OwnerId == userId
                : db.Memberships.Any(membership => membership.ProjectId == project.Id
                    && membership.UserId == userId
                    && (membership.Role == "projectAdmin" || membership.Role == "member")),
            CanManageMembers = project.Kind == "team" && db.Memberships.Any(membership =>
                membership.ProjectId == project.Id && membership.UserId == userId
                && membership.Role == "projectAdmin")
        })
        .ToListAsync(ct);
    return Results.Ok(projects);
}).RequireAuthorization();

app.MapPost("/api/projects", async (
    CreateTeamProjectInput input, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    if (!principal.IsInRole("serverAdmin")) return Results.Forbid();
    if (input is null || input.Name is null) return Results.BadRequest();
    var name = input.Name.Trim();
    if (name.Length is < 1 or > 200) return Results.BadRequest();
    var id = Guid.NewGuid();
    var snapshot = System.Text.Json.JsonSerializer.Serialize(new
    {
        project = new { schemaVersion = 1, id, name },
        collections = Array.Empty<object>(),
        environments = new { schemaVersion = 1, environments = Array.Empty<object>(), variables = Array.Empty<object>() }
    });
    var ownerId = UserId(principal);
    var project = new ProjectRecord { Id = id, OwnerId = ownerId, Name = name, Kind = "team", Snapshot = snapshot };
    db.Projects.Add(project);
    db.Memberships.Add(new ProjectMembership { ProjectId = id, UserId = ownerId, Role = "projectAdmin" });
    await db.SaveChangesAsync(ct);
    return Results.Created($"/api/projects/{id}", ToView(project));
}).RequireAuthorization();

app.MapGet("/api/projects/{id:guid}/members", async (
    Guid id, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var project = await db.Projects.SingleOrDefaultAsync(candidate => candidate.Id == id && candidate.Kind == "team", ct);
    if (project is null || !await IsProjectAdmin(principal, project, db, ct)) return Results.NotFound();
    var members = await db.Memberships.Where(membership => membership.ProjectId == id)
        .Join(db.Users, membership => membership.UserId, user => user.Id,
            (membership, user) => new { user.Id, user.UserName, membership.Role })
        .OrderBy(member => member.UserName).ToListAsync(ct);
    return Results.Ok(members);
}).RequireAuthorization();

app.MapPost("/api/projects/{id:guid}/members", async (
    Guid id, MembershipInput input, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var project = await db.Projects.SingleOrDefaultAsync(candidate => candidate.Id == id && candidate.Kind == "team", ct);
    if (project is null || !await IsProjectAdmin(principal, project, db, ct)) return Results.NotFound();
    if (input is null || input.Username is null || input.Role is null) return Results.BadRequest();
    var username = input.Username.Trim();
    var user = await db.Users.SingleOrDefaultAsync(candidate => candidate.UserName == username, ct);
    if (user is null || user.Status != "active") return Results.NotFound();
    if (input.Role is not ("member" or "projectAdmin")) return Results.BadRequest();
    if (await db.Memberships.AnyAsync(item => item.ProjectId == id && item.UserId == user.Id, ct))
        return Results.Conflict(new { error = "User is already a project member." });
    db.Memberships.Add(new ProjectMembership { ProjectId = id, UserId = user.Id, Role = input.Role });
    await db.SaveChangesAsync(ct);
    return Results.NoContent();
}).RequireAuthorization();

app.MapPut("/api/projects/{id:guid}/members/{memberId:guid}", async (
    Guid id, Guid memberId, MembershipInput input, ClaimsPrincipal principal, FluxkitDb db,
    CancellationToken ct) =>
{
    var project = await db.Projects.SingleOrDefaultAsync(candidate => candidate.Id == id && candidate.Kind == "team", ct);
    if (project is null || !await IsProjectAdmin(principal, project, db, ct)) return Results.NotFound();
    if (input is null || input.Role is null) return Results.BadRequest();
    if (input.Role is not ("member" or "projectAdmin")) return Results.BadRequest();
    var membership = await db.Memberships.SingleOrDefaultAsync(item =>
        item.ProjectId == id && item.UserId == memberId, ct);
    if (membership is null) return Results.NotFound();
    if (membership.Role == "projectAdmin" && input.Role != "projectAdmin"
        && !await db.Memberships.AnyAsync(item => item.ProjectId == id && item.UserId != memberId
            && item.Role == "projectAdmin", ct))
        return Results.Conflict(new { error = "A team project must keep at least one project administrator." });
    membership.Role = input.Role;
    await db.SaveChangesAsync(ct);
    return Results.NoContent();
}).RequireAuthorization();

app.MapDelete("/api/projects/{id:guid}/members/{memberId:guid}", async (
    Guid id, Guid memberId, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var project = await db.Projects.SingleOrDefaultAsync(candidate => candidate.Id == id && candidate.Kind == "team", ct);
    if (project is null || !await IsProjectAdmin(principal, project, db, ct)) return Results.NotFound();
    if (project.OwnerId == memberId) return Results.Conflict(new { error = "The project owner cannot be removed." });
    if (!await db.Memberships.AnyAsync(item => item.ProjectId == id && item.UserId != memberId
        && item.Role == "projectAdmin", ct))
        return Results.Conflict(new { error = "A team project must keep at least one project administrator." });
    await db.Memberships.Where(item => item.ProjectId == id && item.UserId == memberId).ExecuteDeleteAsync(ct);
    return Results.NoContent();
}).RequireAuthorization();

app.MapGet("/api/projects/{id:guid}", async (
    Guid id, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var project = await AuthorizedProject(id, UserId(principal), db, ct);
    if (project is null) return Results.NotFound();
    var canEdit = await CanEdit(principal, project, db, ct);
    return Results.Ok(new
    {
        project.Id,
        project.Name,
        project.Kind,
        project.Snapshot,
        project.Revision,
        canEdit,
        etag = $"\"{project.Revision}\""
    });
}).RequireAuthorization();

app.MapPost("/api/projects/{id:guid}/takeover", async (
    Guid id, TakeoverInput input, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var userId = UserId(principal);
    if (input is null || input.Snapshot is null || input.Name is null || input.Kind is null
        || input.EncryptedSecrets is null) return Results.BadRequest();
    if (Encoding.UTF8.GetByteCount(input.Snapshot) > 32 * 1024 * 1024
        || input.EncryptedSecrets.Count > 1000)
        return Results.BadRequest(new { error = "Project snapshot exceeds the server limit." });
    if (!TryReadSnapshot(input.Snapshot, id, out var snapshotName)
        || !string.Equals(snapshotName, input.Name.Trim(), StringComparison.Ordinal))
        return Results.BadRequest(new { error = "Project snapshot identity or name is invalid." });
    var existing = await db.Projects.SingleOrDefaultAsync(project => project.Id == id, ct);
    if (existing is not null) return Results.Conflict(new { error = "Project identity already exists on this server." });
    if (input.Kind is not ("personal" or "team")) return Results.BadRequest();
    if (input.Kind == "team" && !principal.IsInRole("serverAdmin")) return Results.Forbid();
    var project = new ProjectRecord
    {
        Id = id,
        OwnerId = userId,
        Name = input.Name.Trim(),
        Kind = input.Kind,
        Snapshot = input.Snapshot,
        Revision = 1
    };
    if (project.Name.Length is < 1 or > 200) return Results.BadRequest();
    var shared = input.SharedValues ?? [];
    var encrypted = input.VaultValues ?? [];
    if (!ValueEndpoints.ValidShared(input.Snapshot, shared)
        || !ValueEndpoints.ValidSecrets(input.Snapshot, encrypted)
        || (encrypted.Count > 0 && !await db.AccountVaults.AnyAsync(vault => vault.UserId == userId, ct)))
    {
        return Results.BadRequest(new { error = "Invalid value transfer. Secrets require a configured account vault." });
    }
    project.SharedValues = ValueEndpoints.Encode(shared);
    project.SharedValuesRevision = shared.Count == 0 ? 0 : 1;
    db.Projects.Add(project);
    if (encrypted.Count > 0)
    {
        db.ProjectSecrets.Add(new ProjectSecrets { ProjectId = id, UserId = userId, Values = ValueEndpoints.Encode(encrypted) });
    }
    if (project.Kind == "team")
        db.Memberships.Add(new ProjectMembership { ProjectId = id, UserId = userId, Role = "projectAdmin" });
    foreach (var secret in input.EncryptedSecrets)
    {
        if (!ValidEnvelope(secret)) return Results.BadRequest();
        db.EncryptedSecrets.Add(ToEntity(id, secret, userId));
    }
    try { await db.SaveChangesAsync(ct); }
    catch (DbUpdateException) { return Results.Conflict(new { error = "Project takeover could not be committed." }); }
    return Results.Created($"/api/projects/{id}", ToView(project));
}).RequireAuthorization();

app.MapPut("/api/projects/{id:guid}", async (
    Guid id, UpdateProjectInput input, ClaimsPrincipal principal, FluxkitDb db,
    HttpRequest request, HttpResponse response, CancellationToken ct) =>
{
    var project = await AuthorizedProject(id, UserId(principal), db, ct);
    if (project is null) return Results.NotFound();
    if (!await CanEdit(principal, project, db, ct)) return Results.Forbid();
    if (!TryRevision(request, project.Revision, out var conflict))
        return conflict!;
    if (input is null || input.Snapshot is null) return Results.BadRequest();
    if (Encoding.UTF8.GetByteCount(input.Snapshot) > 32 * 1024 * 1024
        || !TryReadSnapshot(input.Snapshot, id, out var snapshotName)) return Results.BadRequest();
    var expectedRevision = project.Revision;
    var updatedAt = DateTimeOffset.UtcNow;
    // Compare and replace in one database statement, including competing requests that
    // passed the HTTP precondition check before either of them committed.
    var changed = await db.Projects
        .Where(candidate => candidate.Id == id && candidate.Revision == expectedRevision)
        .ExecuteUpdateAsync(update => update
            .SetProperty(candidate => candidate.Snapshot, input.Snapshot)
            .SetProperty(candidate => candidate.Name, snapshotName)
            .SetProperty(candidate => candidate.Revision, expectedRevision + 1)
            .SetProperty(candidate => candidate.UpdatedAt, updatedAt), ct);
    if (changed == 0)
        return Results.Conflict(new { error = "Project changed on the server. Reload before saving." });
    project.Snapshot = input.Snapshot;
    project.Name = snapshotName;
    project.Revision = expectedRevision + 1;
    project.UpdatedAt = updatedAt;
    response.Headers.ETag = $"\"{project.Revision}\"";
    return Results.Ok(ToView(project));
}).RequireAuthorization();

app.MapGet("/api/projects/{id:guid}/secrets", async (
    Guid id, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var userId = UserId(principal);
    if (await AuthorizedProject(id, userId, db, ct) is null) return Results.NotFound();
    var values = await db.EncryptedSecrets.AsNoTracking()
        .Where(secret => secret.ProjectId == id && secret.UserId == userId)
        .Select(secret => new SecretEnvelope(secret.VariableId, secret.EnvironmentId, secret.Salt,
            secret.Nonce, secret.Ciphertext, secret.WrappedKey, secret.WrappedKeyNonce,
            secret.Algorithm, secret.KdfParameters))
        .ToListAsync(ct);
    return Results.Ok(values);
}).RequireAuthorization();

app.MapPut("/api/projects/{id:guid}/secrets", async (
    Guid id, SecretEnvelope value, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    var userId = UserId(principal);
    if (await AuthorizedProject(id, userId, db, ct) is null) return Results.NotFound();
    if (!ValidEnvelope(value)) return Results.BadRequest(new { error = "Invalid encrypted secret envelope." });
    var row = await db.EncryptedSecrets.SingleOrDefaultAsync(secret =>
        secret.ProjectId == id && secret.UserId == userId && secret.VariableId == value.VariableId
        && secret.EnvironmentId == value.EnvironmentId, ct);
    if (row is null) db.EncryptedSecrets.Add(ToEntity(id, value, userId));
    else
    {
        row.Salt = value.Salt; row.Nonce = value.Nonce; row.Ciphertext = value.Ciphertext;
        row.WrappedKey = value.WrappedKey; row.WrappedKeyNonce = value.WrappedKeyNonce;
        row.Algorithm = value.Algorithm;
        row.KdfParameters = value.KdfParameters;
    }
    await db.SaveChangesAsync(ct);
    return Results.NoContent();
}).RequireAuthorization();

app.MapDelete("/api/projects/{id:guid}/secrets/{variableId:guid}/{environmentId}", async (
    Guid id, Guid variableId, string environmentId, ClaimsPrincipal principal, FluxkitDb db,
    CancellationToken ct) =>
{
    var userId = UserId(principal);
    if (await AuthorizedProject(id, userId, db, ct) is null) return Results.NotFound();
    await db.EncryptedSecrets.Where(secret => secret.ProjectId == id && secret.UserId == userId
        && secret.VariableId == variableId && secret.EnvironmentId == environmentId)
        .ExecuteDeleteAsync(ct);
    return Results.NoContent();
}).RequireAuthorization();

await using (var scope = app.Services.CreateAsyncScope())
{
    var db = scope.ServiceProvider.GetRequiredService<FluxkitDb>();
    await db.Database.MigrateAsync();
    var config = scope.ServiceProvider.GetRequiredService<IConfiguration>();
    var adminName = config["Bootstrap:AdminUsername"];
    var adminPassword = config["Bootstrap:AdminPassword"];
    if (mode == "local" && !string.IsNullOrWhiteSpace(adminName) && !string.IsNullOrWhiteSpace(adminPassword))
    {
        var users = scope.ServiceProvider.GetRequiredService<UserManager<FluxkitUser>>();
        var admin = await users.FindByNameAsync(adminName);
        if (admin is null)
        {
            admin = new FluxkitUser { Id = Guid.NewGuid(), UserName = adminName, Status = "active", IsServerAdmin = true };
            var result = await users.CreateAsync(admin, adminPassword);
            if (!result.Succeeded)
            {
                var errors = string.Join("; ", result.Errors.Select(error => error.Description));
                throw new InvalidOperationException($"Bootstrap administrator could not be created: {errors}");
            }
            db.Projects.Add(AuthTokens.CreatePersonalProject(admin.Id, adminName));
            await db.SaveChangesAsync();
        }
        else if (!admin.IsServerAdmin || admin.Status != "active")
        {
            throw new InvalidOperationException("The configured bootstrap username exists but is not an active server administrator.");
        }
    }
}
app.Run();

static bool IsExampleSecret(string value) => value.Contains("CHANGE_ME", StringComparison.OrdinalIgnoreCase);

static Guid UserId(ClaimsPrincipal principal) => Guid.Parse(principal.FindFirstValue(JwtRegisteredClaimNames.Sub)!);

static async Task<ProjectRecord?> AuthorizedProject(Guid id, Guid userId, FluxkitDb db, CancellationToken ct) =>
    await db.Projects.SingleOrDefaultAsync(project => project.Id == id &&
        (project.OwnerId == userId || db.Memberships.Any(membership =>
            membership.ProjectId == id && membership.UserId == userId)), ct);

static async Task<bool> CanEdit(ClaimsPrincipal principal, ProjectRecord project, FluxkitDb db, CancellationToken ct)
{
    var userId = UserId(principal);
    if (project.Kind == "personal") return project.OwnerId == userId;
    return await db.Memberships.AnyAsync(membership => membership.ProjectId == project.Id
        && membership.UserId == userId && (membership.Role == "projectAdmin" || membership.Role == "member"), ct);
}

static async Task<bool> IsProjectAdmin(ClaimsPrincipal principal, ProjectRecord project, FluxkitDb db, CancellationToken ct)
{
    var userId = UserId(principal);
    return project.OwnerId == userId || await db.Memberships.AnyAsync(membership =>
        membership.ProjectId == project.Id && membership.UserId == userId && membership.Role == "projectAdmin", ct);
}

static bool TryRevision(HttpRequest request, long current, out IResult? error)
{
    error = null;
    if (!request.Headers.TryGetValue("If-Match", out var header)
        || !long.TryParse(header.ToString().Trim('"'), out var expected))
    {
        error = Results.StatusCode(StatusCodes.Status428PreconditionRequired);
        return false;
    }
    if (expected != current)
    {
        error = Results.Conflict(new { error = "Project changed on the server. Reload before saving.", currentRevision = current });
        return false;
    }
    return true;
}

static object ToView(ProjectRecord project) => new
{
    project.Id,
    project.Name,
    project.Kind,
    project.Snapshot,
    project.Revision,
    etag = $"\"{project.Revision}\""
};

static bool TryReadSnapshot(string snapshot, Guid projectId, out string name)
{
    name = "";
    try
    {
        using var document = System.Text.Json.JsonDocument.Parse(snapshot);
        var root = document.RootElement;
        var project = root.GetProperty("project");
        if (!Guid.TryParse(project.GetProperty("id").GetString(), out var id) || id != projectId)
            return false;
        name = project.GetProperty("name").GetString() ?? "";
        if (name.Trim().Length is < 1 or > 200) return false;
        return root.GetProperty("collections").ValueKind == System.Text.Json.JsonValueKind.Array
            && root.GetProperty("environments").ValueKind == System.Text.Json.JsonValueKind.Object;
    }
    catch (Exception exception) when (exception is System.Text.Json.JsonException
        or InvalidOperationException or KeyNotFoundException or FormatException)
    {
        return false;
    }
}


static bool ValidEnvelope(SecretEnvelope value) =>
    value is not null && value.EnvironmentId is not null && value.Salt is not null
    && value.Nonce is not null && value.Ciphertext is not null && value.WrappedKey is not null
    && value.WrappedKeyNonce is not null && value.KdfParameters is not null
    && value.Algorithm == "AES-256-GCM" && value.VariableId != Guid.Empty
    && value.EnvironmentId.Length <= 64 && IsBase64(value.Salt, 16)
    && IsBase64(value.Nonce, 12) && IsBase64(value.WrappedKeyNonce, 12)
    && IsBase64(value.Ciphertext, 16, 16_384) && IsBase64(value.WrappedKey, 48, 48)
    && ValidKdfParameters(value.KdfParameters);

static EncryptedSecretRecord ToEntity(Guid projectId, SecretEnvelope value, Guid userId) => new()
{
    ProjectId = projectId,
    UserId = userId,
    VariableId = value.VariableId,
    EnvironmentId = value.EnvironmentId,
    Salt = value.Salt,
    Nonce = value.Nonce,
    Ciphertext = value.Ciphertext,
    WrappedKey = value.WrappedKey,
    Algorithm = value.Algorithm,
    WrappedKeyNonce = value.WrappedKeyNonce,
    KdfParameters = value.KdfParameters
};

static bool IsBase64(string value, int minimumBytes, int maximumBytes = int.MaxValue)
{
    try
    {
        var bytes = Convert.FromBase64String(value);
        return bytes.Length >= minimumBytes && bytes.Length <= maximumBytes;
    }
    catch (FormatException) { return false; }
}

static bool ValidKdfParameters(string value)
{
    if (value.Length > 1024) return false;
    try
    {
        using var document = System.Text.Json.JsonDocument.Parse(value);
        var root = document.RootElement;
        return root.ValueKind == System.Text.Json.JsonValueKind.Object
            && root.GetProperty("algorithm").GetString() == "Argon2id"
            && root.GetProperty("version").GetInt32() == 19
            && root.GetProperty("memoryKiB").GetInt32() is >= 65_536 and <= 262_144
            && root.GetProperty("iterations").GetInt32() is >= 3 and <= 10
            && root.GetProperty("parallelism").GetInt32() is >= 1 and <= 4;
    }
    catch (Exception exception) when (exception is System.Text.Json.JsonException
        or InvalidOperationException or KeyNotFoundException or FormatException)
    {
        return false;
    }
}

public partial class Program { }

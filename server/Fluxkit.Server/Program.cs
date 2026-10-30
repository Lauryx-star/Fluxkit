using Microsoft.AspNetCore.HttpOverrides;
using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using System.Threading.RateLimiting;
using Fluxkit.Server;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.Identity;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;
using Prometheus;

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
AuthTokens.Validate(builder.Configuration);
// Behind a reverse proxy every request comes from the proxy's address. Trusting X-Forwarded-For
// from configured networks restores the client address (for rate limits); only the last hop,
// the one the trusted proxy appended, is used.
var trustedProxies = (builder.Configuration["Proxy:TrustedNetworks"] ?? "")
    .Split([',', ';', ' '], StringSplitOptions.RemoveEmptyEntries | StringSplitOptions.TrimEntries)
    .Select(value => System.Net.IPNetwork.TryParse(value, out var network)
        ? network
        : throw new InvalidOperationException($"Proxy:TrustedNetworks contains an invalid network: {value}"))
    .ToList();
if (trustedProxies.Count > 0)
{
    builder.Services.Configure<ForwardedHeadersOptions>(options =>
    {
        options.ForwardedHeaders = ForwardedHeaders.XForwardedFor;
        options.ForwardLimit = 1;
        options.KnownProxies.Clear();
        options.KnownIPNetworks.Clear();
        foreach (var network in trustedProxies) { options.KnownIPNetworks.Add(network); }
    });
}

builder.Services.AddDbContext<FluxkitDb>(options => options.UseNpgsql(connection));
builder.Services.AddSingleton<UsageMetrics>();
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
// Clients show 429 as "too many attempts"; the framework default would be 503.
builder.Services.AddRateLimiter(options => options.RejectionStatusCode = StatusCodes.Status429TooManyRequests);
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

builder.Services.AddRateLimiter(options => options.AddPolicy("refresh", context =>
    RateLimitPartition.GetFixedWindowLimiter(context.Connection.RemoteIpAddress?.ToString() ?? "unknown",
        _ => new FixedWindowRateLimiterOptions { PermitLimit = 120, Window = TimeSpan.FromMinutes(1), QueueLimit = 0 })));
builder.Services.AddRateLimiter(options => options.AddPolicy("oidc-poll", context =>
    RateLimitPartition.GetFixedWindowLimiter(context.Connection.RemoteIpAddress?.ToString() ?? "unknown",
        _ => new FixedWindowRateLimiterOptions { PermitLimit = 120, Window = TimeSpan.FromMinutes(1), QueueLimit = 0 })));
var app = builder.Build();
var usage = app.Services.GetRequiredService<UsageMetrics>();
app.UseExceptionHandler();
if (trustedProxies.Count > 0) { app.UseForwardedHeaders(); }
app.UseRouting();
// Request counts and durations per route template (never per concrete path or identifier).
app.UseHttpMetrics(options => options.SetMetricFactory(Metrics.WithCustomRegistry(usage.Registry)));
app.UseRateLimiter();
app.UseAuthentication();
app.UseAuthorization();
app.Use(async (context, next) =>
{
    if (context.User.FindFirstValue(JwtRegisteredClaimNames.Sub) is { } subject && Guid.TryParse(subject, out var account))
    {
        usage.Seen(account);
    }
    await next();
});
if (app.Configuration.GetValue("Metrics:Enabled", true))
{
    // Prometheus scrape endpoint without login; restrict access at the reverse proxy.
    app.MapMetrics("/metrics", usage.Registry).AllowAnonymous();
}
ValueEndpoints.Map(app);
ProjectEndpoints.Map(app);
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
        usage.Registration("local");
        return Results.Accepted(value: new { status = "pendingApproval" });
    }).AllowAnonymous().RequireRateLimiting("auth");

    app.MapPost("/api/auth/login", async (
        LoginInput input, UserManager<FluxkitUser> users, SignInManager<FluxkitUser> signIn,
        IConfiguration configuration, FluxkitDb db) =>
    {
        if (input is null || string.IsNullOrWhiteSpace(input.Username) || input.Password is null)
            return Results.Unauthorized();
        var user = await users.FindByNameAsync(input.Username.Trim());
        if (user is null || user.Status != "active")
        {
            usage.Login("local", user?.Status == "pending" ? "pending" : "rejected");
            return Results.Unauthorized();
        }
        var result = await signIn.CheckPasswordSignInAsync(user, input.Password, lockoutOnFailure: true);
        if (!result.Succeeded)
        {
            usage.Login("local", "rejected");
            return Results.Unauthorized();
        }
        usage.Login("local", "success");
        return Results.Ok(await AuthTokens.StartSession(user, configuration, db));
    }).AllowAnonymous().RequireRateLimiting("auth");

}
else { OidcLogin.Map(app); }

// Desktop sessions in both modes: refresh tokens rotate on every use and expire after inactivity.
app.MapPost("/api/auth/refresh", async (RefreshInput input, IConfiguration configuration, FluxkitDb db,
    HttpResponse response, CancellationToken ct) =>
{
    response.Headers.CacheControl = "no-store";
    var reply = await AuthTokens.Refresh(input?.RefreshToken, configuration, db, ct);
    return reply is null ? Results.Unauthorized() : Results.Ok(reply);
}).AllowAnonymous().RequireRateLimiting("refresh");
app.MapPost("/api/auth/logout", async (RefreshInput input, FluxkitDb db, CancellationToken ct) =>
{
    await AuthTokens.Revoke(input?.RefreshToken, db, ct);
    return Results.NoContent();
}).AllowAnonymous().RequireRateLimiting("refresh");

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
        .Select(user => new
        {
            user.Id,
            user.UserName,
            user.Status,
            user.IsServerAdmin,
            user.CreatedAt,
            // What deleting the account removes, so the confirmation can say so.
            PersonalCollections = db.Collections.Count(collection => db.Projects.Any(project =>
                project.Id == collection.ProjectId && project.OwnerId == user.Id && project.Kind == "personal")),
            TeamProjects = db.Memberships.Count(membership => membership.UserId == user.Id)
        })
        .ToListAsync(ct)))
    .RequireAuthorization(policy => policy.RequireRole("serverAdmin"));

app.MapPost("/api/admin/users/{id:guid}/approve", async (
    Guid id, FluxkitDb db, CancellationToken ct) =>
{
    var user = await db.Users.SingleOrDefaultAsync(candidate => candidate.Id == id, ct);
    if (user is null) return Results.NotFound();
    if (user.Status != "pending") return Results.Conflict(new { error = "Only pending accounts can be approved." });
    user.Status = "active";
    usage.AccountChange("approved");
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
    if (user.Status != input.Status) { usage.AccountChange(input.Status == "active" ? "reactivated" : "disabled"); }
    user.Status = input.Status;
    await db.SaveChangesAsync(ct);
    if (input.Status != "active")
    {
        // Disabled accounts lose their desktop sessions; access tokens already fail validation.
        await db.RefreshSessions.Where(session => session.UserId == id).ExecuteDeleteAsync(ct);
    }
    return Results.NoContent();
}).RequireAuthorization(policy => policy.RequireRole("serverAdmin"));

// Permanently removes an account with its private data: the personal project (with all its
// rows and values), the account vault, its own encrypted values in every project and its team
// memberships. Team projects and their content stay; rows it last changed keep the stale id.
app.MapDelete("/api/admin/users/{id:guid}", async (
    Guid id, ClaimsPrincipal principal, FluxkitDb db, CancellationToken ct) =>
{
    if (id == UserId(principal))
        return Results.Conflict(new { error = "You cannot delete your own account." });
    await using var transaction = await db.Database.BeginTransactionAsync(System.Data.IsolationLevel.Serializable, ct);
    var user = await db.Users.SingleOrDefaultAsync(candidate => candidate.Id == id, ct);
    if (user is null) return Results.NotFound();
    if (user.IsServerAdmin && user.Status == "active"
        && !await db.Users.AnyAsync(candidate => candidate.Id != id && candidate.IsServerAdmin
            && candidate.Status == "active", ct))
        return Results.Conflict(new { error = "At least one active server administrator must remain." });
    await db.Projects.Where(project => project.OwnerId == id && project.Kind == "personal").ExecuteDeleteAsync(ct);
    // Vault, encrypted values, memberships and identity rows cascade with the account.
    await db.Users.Where(candidate => candidate.Id == id).ExecuteDeleteAsync(ct);
    await transaction.CommitAsync(ct);
    usage.AccountChange("deleted");
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
            project.CreatedAt,
            CollectionCount = db.Collections.Count(collection => collection.ProjectId == project.Id),
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
        project.Revision,
        project.CreatedAt,
        canEdit,
        etag = $"\"{project.Revision}\""
    });
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

static object ToView(ProjectRecord project) => new
{
    project.Id,
    project.Name,
    project.Kind,
    project.Revision,
    project.CreatedAt,
    etag = $"\"{project.Revision}\""
};

public partial class Program { }

using System.Security.Cryptography;
using System.Text;
using Microsoft.AspNetCore.Authentication;
using Microsoft.AspNetCore.Authentication.OpenIdConnect;
using Microsoft.AspNetCore.Identity;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Protocols.OpenIdConnect;

namespace Fluxkit.Server;

/// Server-held confidential OIDC client; the desktop receives only a Fluxkit session.
public static class OidcLogin
{
    private const string Scheme = "oidc";
    private const string TicketKey = "fluxkit-ticket";
    public static void Configure(WebApplicationBuilder builder)
    {
        var config = builder.Configuration;
        var issuer = Required(config, "Auth:Oidc:Authority");
        var origin = Required(config, "Auth:Oidc:PublicOrigin");
        foreach (var value in new[] { issuer, origin })
        {
            if (!Uri.TryCreate(value, UriKind.Absolute, out var uri) || uri.Scheme != "https"
                || uri.UserInfo.Length != 0 || uri.Query.Length != 0 || uri.Fragment.Length != 0)
            { throw new InvalidOperationException("OIDC authority and public origin must be HTTPS URLs without credentials, query or fragment."); }
        }
        if (new Uri(origin).AbsolutePath != "/")
        { throw new InvalidOperationException("OIDC PublicOrigin must be an origin without a path prefix."); }
        var clientId = Required(config, "Auth:Oidc:ClientId");
        var secret = Required(config, "Auth:Oidc:ClientSecret");
        _ = Required(config, "Bootstrap:OidcSubject");
        builder.Services.AddSingleton<OidcTickets>();
        builder.Services.AddAuthentication().AddCookie("oidc-unused")
            .AddOpenIdConnect(Scheme, options =>
            {
                options.SignInScheme = "oidc-unused";
                options.Authority = issuer;
                options.ClientId = clientId;
                options.ClientSecret = secret;
                options.CallbackPath = "/api/auth/oidc/callback";
                options.ResponseType = OpenIdConnectResponseType.Code;
                options.ResponseMode = OpenIdConnectResponseMode.FormPost;
                options.UsePkce = true;
                options.RequireHttpsMetadata = true;
                options.BackchannelTimeout = TimeSpan.FromSeconds(15);
                options.SaveTokens = false;
                options.MapInboundClaims = false;
                options.GetClaimsFromUserInfoEndpoint = false;
                options.Scope.Clear(); options.Scope.Add("openid"); options.Scope.Add("profile");
                options.TokenValidationParameters.ValidIssuer = issuer;
                options.TokenValidationParameters.ClockSkew = TimeSpan.FromSeconds(30);
                options.TokenValidationParameters.NameClaimType = "preferred_username";
                options.RemoteAuthenticationTimeout = TimeSpan.FromMinutes(5);
                // This is an operator-configured address, never derived from Host or forwarded headers.
                options.Events = new OpenIdConnectEvents
                {
                    OnRedirectToIdentityProvider = context =>
                    {
                        context.ProtocolMessage.RedirectUri = origin.TrimEnd('/') + "/api/auth/oidc/callback";
                        return Task.CompletedTask;
                    },
                    OnAuthorizationCodeReceived = context =>
                    {
                        context.TokenEndpointRequest!.RedirectUri = origin.TrimEnd('/') + "/api/auth/oidc/callback";
                        return Task.CompletedTask;
                    },
                    OnTokenValidated = context =>
                    {
                        // Default claim actions remove protocol claims such as iss from the principal.
                        context.Properties!.Items["fluxkit-issuer"] = context.SecurityToken.Issuer;
                        return Task.CompletedTask;
                    },
                    // TicketReceived runs after signature, issuer, audience, lifetime, nonce and correlation checks.
                    OnTicketReceived = async context =>
                    {
                        var tickets = context.HttpContext.RequestServices.GetRequiredService<OidcTickets>();
                        var ticket = (context.Properties?.Items.TryGetValue(TicketKey, out var ticketId) == true ? ticketId : null);
                        if (ticket is not null && tickets.IsWaiting(ticket))
                        {
                            var db = context.HttpContext.RequestServices.GetRequiredService<FluxkitDb>();
                            var subject = context.Principal?.FindFirst("sub")?.Value;
                            var tokenIssuer = context.Properties?.Items.TryGetValue("fluxkit-issuer", out var verifiedIssuer) == true ? verifiedIssuer : null;
                            if (tokenIssuer == issuer && subject is { Length: > 0 and <= 255 })
                            {
                                var user = await Enroll(db, issuer, subject,
                                    context.Principal?.FindFirst("preferred_username")?.Value,
                                    config["Bootstrap:OidcSubject"]!,
                                    context.HttpContext.RequestServices.GetService<UsageMetrics>());
                                tickets.Complete(ticket, user.Id);
                            }
                            else { tickets.Fail(ticket); }
                        }
                        context.HandleResponse();
                        await BrowserReply(context.Response);
                    },
                    OnRemoteFailure = async context =>
                    {
                        var ticket = (context.Properties?.Items.TryGetValue(TicketKey, out var ticketId) == true ? ticketId : null);
                        if (ticket is not null) { context.HttpContext.RequestServices.GetRequiredService<OidcTickets>().Fail(ticket); }
                        context.HandleResponse();
                        await BrowserReply(context.Response);
                    }
                };
            });
    }
    private static string Required(IConfiguration config, string key)
    {
        var value = config[key];
        if (string.IsNullOrWhiteSpace(value) || value.Contains("CHANGE_ME", StringComparison.OrdinalIgnoreCase))
        { throw new InvalidOperationException($"Configure {key} before starting OIDC mode."); }
        return value;
    }
    private static Task BrowserReply(HttpResponse response)
    {
        response.Headers.CacheControl = "no-store";
        response.Headers.ContentSecurityPolicy = "default-src 'none'; frame-ancestors 'none'";
        response.Headers["Referrer-Policy"] = "no-referrer";
        response.ContentType = "text/plain; charset=utf-8";
        return response.WriteAsync("Return to Fluxkit to see the sign-in result. You can close this tab.");
    }
    private static string Hash(string value) => Convert.ToHexString(SHA256.HashData(Encoding.UTF8.GetBytes(value)));
    internal static async Task<FluxkitUser> Enroll(FluxkitDb db, string issuer, string subject, string? display, string bootstrap, UsageMetrics? usage = null)
    {
        var provider = "oidc:" + Hash(issuer);
        var key = Hash(subject);
        async Task<FluxkitUser?> Existing() => await db.UserLogins.Where(login => login.LoginProvider == provider && login.ProviderKey == key)
            .Join(db.Users, login => login.UserId, user => user.Id, (_, user) => user).SingleOrDefaultAsync();
        var existing = await Existing();
        if (existing is not null) { return existing; }
        var id = Guid.NewGuid();
        var prefix = new string((display ?? "oidc").Where(character => char.IsAsciiLetterOrDigit(character) || character is '-' or '_').Take(40).ToArray());
        if (prefix.Length == 0) { prefix = "oidc"; }
        var name = $"{prefix}-{id:N}";
        var admin = string.Equals(subject, bootstrap, StringComparison.Ordinal);
        var user = new FluxkitUser
        {
            Id = id,
            UserName = name,
            NormalizedUserName = name.ToUpperInvariant(),
            Status = admin ? "active" : "pending",
            IsServerAdmin = admin
        };
        db.Users.Add(user);
        db.UserLogins.Add(new IdentityUserLogin<Guid>
        {
            LoginProvider = provider,
            ProviderKey = key,
            ProviderDisplayName = "OpenID Connect",
            UserId = id
        });
        if (admin) { db.Projects.Add(AuthTokens.CreatePersonalProject(id, name)); }
        try
        {
            await db.SaveChangesAsync();
            usage?.Registration("oidc");
            return user;
        }
        catch (DbUpdateException)
        {
            // Two valid callbacks for one new identity must converge on the same account.
            db.ChangeTracker.Clear();
            return await Existing() ?? throw new InvalidOperationException("OIDC enrollment could not be saved.");
        }
    }
    public static void Map(WebApplication app)
    {
        app.MapPost("/api/auth/oidc/start", (OidcTickets tickets, HttpResponse response) =>
        {
            response.Headers.CacheControl = "no-store";
            var ticket = tickets.Start();
            return ticket is null ? Results.StatusCode(503) : Results.Ok(ticket);
        }).AllowAnonymous().RequireRateLimiting("auth");
        app.MapGet("/api/auth/oidc/authorize/{id:guid}", (Guid id, OidcTickets tickets) =>
            tickets.Claim(id.ToString()) ? Results.Challenge(new AuthenticationProperties
            {
                Items = { [TicketKey] = id.ToString() }
            }, [Scheme]) : Results.BadRequest())
            .AllowAnonymous().RequireRateLimiting("auth");
        app.MapPost("/api/auth/oidc/poll", async (OidcPoll input, OidcTickets tickets, FluxkitDb db,
            IConfiguration config, HttpResponse response, UsageMetrics usage) =>
        {
            response.Headers.CacheControl = "no-store";
            var result = tickets.Poll(input.Id, input.Secret);
            if (result.Status == "complete")
            {
                var user = await db.Users.AsNoTracking().SingleOrDefaultAsync(user => user.Id == result.UserId);
                if (user?.Status == "active")
                {
                    usage.Login("oidc", "success");
                    return Results.Ok(new { status = "complete", session = AuthTokens.Create(user, config) });
                }
                usage.Login("oidc", user?.Status == "pending" ? "pending" : "rejected");
                return Results.Ok(new { status = user?.Status == "pending" ? "pendingApproval" : "denied" });
            }
            return Results.Ok(new { status = result.Status });
        }).AllowAnonymous().RequireRateLimiting("oidc-poll");
    }
}
public sealed record OidcPoll(string Id, string Secret);
public sealed record OidcStart(string Id, string Secret, int ExpiresIn = 300);

/// Bounded, single-process handoffs. Secrets never appear in browser URLs or logs.
public sealed class OidcTickets(TimeProvider? timeProvider = null)
{
    private readonly TimeProvider time = timeProvider ?? TimeProvider.System;
    private sealed class Ticket(byte[] secret, DateTimeOffset expires)
    {
        public byte[] Secret { get; } = secret;
        public DateTimeOffset Expires { get; } = expires;
        public bool Claimed { get; set; }
        public string Status { get; set; } = "waiting";
        public Guid? User { get; set; }
    }
    private readonly Dictionary<string, Ticket> tickets = [];
    private void Prune()
    {
        foreach (var key in tickets.Where(item => item.Value.Expires <= time.GetUtcNow()).Select(item => item.Key).ToArray())
        { tickets.Remove(key); }
    }
    public OidcStart? Start()
    {
        lock (tickets)
        {
            Prune(); if (tickets.Count >= 256) { return null; }
            var id = Guid.NewGuid().ToString(); var secret = RandomNumberGenerator.GetBytes(32);
            tickets.Add(id, new(secret, time.GetUtcNow().AddMinutes(5))); return new(id, Convert.ToBase64String(secret));
        }
    }
    public bool Claim(string id)
    {
        lock (tickets)
        {
            Prune();
            if (!tickets.TryGetValue(id, out var ticket) || ticket.Claimed) { return false; }
            ticket.Claimed = true; return true;
        }
    }
    public bool IsWaiting(string id)
    {
        lock (tickets) { Prune(); return tickets.TryGetValue(id, out var ticket) && ticket.Claimed && ticket.Status == "waiting"; }
    }
    public void Complete(string id, Guid user) { Finish(id, "complete", user); }
    public void Fail(string id) { Finish(id, "failed", null); }
    private void Finish(string id, string status, Guid? user)
    {
        lock (tickets)
        {
            Prune(); if (tickets.TryGetValue(id, out var ticket) && ticket.Status == "waiting")
            { ticket.Status = status; ticket.User = user; }
        }
    }
    public (string Status, Guid? UserId) Poll(string? id, string? secret)
    {
        if (id is null || secret is null || secret.Length > 44) { return ("expired", null); }
        byte[] bytes;
        try { bytes = Convert.FromBase64String(secret); } catch (FormatException) { return ("expired", null); }
        lock (tickets)
        {
            Prune();
            if (!tickets.TryGetValue(id, out var ticket) || !CryptographicOperations.FixedTimeEquals(bytes, ticket.Secret))
            { return ("expired", null); }
            if (ticket.Status != "waiting") { tickets.Remove(id); }
            return (ticket.Status, ticket.User);
        }
    }
}

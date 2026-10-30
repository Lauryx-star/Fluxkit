using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Security.Cryptography;
using System.Text;
using Microsoft.EntityFrameworkCore;
using Microsoft.IdentityModel.Tokens;

namespace Fluxkit.Server;

/// A desktop sign-in. The refresh token's secret is stored only as a SHA-256 hash and rotates on
/// every use; the session ends after `Auth:SessionIdleDays` without use or `Auth:SessionMaxDays`
/// after sign-in (0 = no absolute limit).
public sealed class RefreshSession
{
    public Guid Id { get; set; }
    public Guid UserId { get; set; }
    public byte[] TokenHash { get; set; } = [];
    public DateTimeOffset CreatedAt { get; set; }
    public DateTimeOffset LastUsedAt { get; set; }
    public DateTimeOffset ExpiresAt { get; set; }
    public DateTimeOffset? AbsoluteExpiresAt { get; set; }
}

public sealed record RefreshInput(string? RefreshToken);

public static class AuthTokens
{
    private static int Setting(IConfiguration config, string key, int fallback, int min, int max)
    {
        var value = config.GetValue(key, fallback);
        return value >= min && value <= max
            ? value
            : throw new InvalidOperationException($"{key} must be between {min} and {max}.");
    }

    /// Validates the session settings at startup so a typo fails fast instead of at sign-in.
    public static void Validate(IConfiguration config)
    {
        _ = Setting(config, "Auth:AccessTokenMinutes", 60, 5, 24 * 60);
        _ = Setting(config, "Auth:SessionIdleDays", 7, 1, 365);
        _ = Setting(config, "Auth:SessionMaxDays", 90, 0, 3650);
    }

    /// Starts a new desktop session for a verified sign-in.
    public static async Task<object> StartSession(FluxkitUser user, IConfiguration config, FluxkitDb db,
        CancellationToken ct = default)
    {
        var now = DateTimeOffset.UtcNow;
        // Expired sessions of this account are removed opportunistically.
        await db.RefreshSessions
            .Where(session => session.UserId == user.Id
                && (session.ExpiresAt < now || session.AbsoluteExpiresAt < now))
            .ExecuteDeleteAsync(ct);
        var maxDays = Setting(config, "Auth:SessionMaxDays", 90, 0, 3650);
        var session = new RefreshSession
        {
            Id = Guid.NewGuid(),
            UserId = user.Id,
            CreatedAt = now,
            AbsoluteExpiresAt = maxDays == 0 ? null : now.AddDays(maxDays)
        };
        var secret = Rotate(session, config, now);
        db.RefreshSessions.Add(session);
        await db.SaveChangesAsync(ct);
        return Reply(user, config, session, secret);
    }

    /// Exchanges a refresh token for a new access token and a new refresh token. Returns null
    /// when the token is unknown, expired, already used, or the account is no longer active.
    public static async Task<object?> Refresh(string? token, IConfiguration config, FluxkitDb db,
        CancellationToken ct = default)
    {
        if (!TryParse(token, out var id, out var secret)) return null;
        var session = await db.RefreshSessions.SingleOrDefaultAsync(item => item.Id == id, ct);
        if (session is null) return null;
        var now = DateTimeOffset.UtcNow;
        var user = await db.Users.AsNoTracking().SingleOrDefaultAsync(item => item.Id == session.UserId, ct);
        if (!CryptographicOperations.FixedTimeEquals(Hash(secret), session.TokenHash)
            || session.ExpiresAt < now || session.AbsoluteExpiresAt < now || user?.Status != "active")
        {
            // A reused (already rotated) token means it may have been copied: end the session.
            db.RefreshSessions.Remove(session);
            await db.SaveChangesAsync(ct);
            return null;
        }
        var next = Rotate(session, config, now);
        try
        {
            await db.SaveChangesAsync(ct);
        }
        catch (DbUpdateConcurrencyException)
        {
            return null;
        }
        return Reply(user, config, session, next);
    }

    /// Ends the session of a refresh token; unknown tokens are ignored.
    public static async Task Revoke(string? token, FluxkitDb db, CancellationToken ct = default)
    {
        if (!TryParse(token, out var id, out var secret)) return;
        var session = await db.RefreshSessions.SingleOrDefaultAsync(item => item.Id == id, ct);
        if (session is not null && CryptographicOperations.FixedTimeEquals(Hash(secret), session.TokenHash))
        {
            db.RefreshSessions.Remove(session);
            await db.SaveChangesAsync(ct);
        }
    }

    private static byte[] Rotate(RefreshSession session, IConfiguration config, DateTimeOffset now)
    {
        var secret = RandomNumberGenerator.GetBytes(32);
        session.TokenHash = Hash(secret);
        session.LastUsedAt = now;
        session.ExpiresAt = now.AddDays(Setting(config, "Auth:SessionIdleDays", 7, 1, 365));
        return secret;
    }

    private static byte[] Hash(byte[] secret) => SHA256.HashData(secret);

    private static bool TryParse(string? token, out Guid id, out byte[] secret)
    {
        id = Guid.Empty;
        secret = [];
        if (token is null || token.Length > 128) return false;
        var parts = token.Split('.');
        if (parts.Length != 2 || !Guid.TryParseExact(parts[0], "N", out id)) return false;
        try
        {
            secret = Base64UrlEncoder.DecodeBytes(parts[1]);
        }
        catch (FormatException)
        {
            return false;
        }
        return secret.Length == 32;
    }

    private static object Reply(FluxkitUser user, IConfiguration config, RefreshSession session, byte[] secret)
    {
        var minutes = Setting(config, "Auth:AccessTokenMinutes", 60, 5, 24 * 60);
        var claims = new List<Claim> {
            new(JwtRegisteredClaimNames.Sub, user.Id.ToString()),
            new(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString()),
            new("auth_mode", (config["Auth:Mode"] ?? "local").ToLowerInvariant()),
            new(ClaimTypes.Name, user.UserName ?? "") };
        if (user.IsServerAdmin) { claims.Add(new("role", "serverAdmin")); }
        var token = new JwtSecurityToken(config["Auth:Issuer"] ?? "fluxkit-self-hosted",
            config["Auth:Audience"] ?? "fluxkit-desktop", claims, DateTime.UtcNow,
            DateTime.UtcNow.AddMinutes(minutes), new SigningCredentials(
                new SymmetricSecurityKey(Encoding.UTF8.GetBytes(config["Auth:SigningKey"]!)),
                SecurityAlgorithms.HmacSha256));
        var lifetime = session.AbsoluteExpiresAt is { } absolute && absolute < session.ExpiresAt
            ? absolute
            : session.ExpiresAt;
        return new
        {
            accessToken = new JwtSecurityTokenHandler().WriteToken(token),
            expiresIn = minutes * 60,
            refreshToken = $"{session.Id:N}.{Base64UrlEncoder.Encode(secret)}",
            refreshExpiresIn = (long)Math.Max(0, (lifetime - DateTimeOffset.UtcNow).TotalSeconds)
        };
    }

    public static ProjectRecord CreatePersonalProject(Guid ownerId, string username)
    {
        var id = Guid.NewGuid();
        var name = $"{username}'s personal project";
        return new ProjectRecord { Id = id, OwnerId = ownerId, Name = name, Kind = "personal", UpdatedBy = ownerId };
    }
}

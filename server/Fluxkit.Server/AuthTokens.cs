using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Text;
using Microsoft.IdentityModel.Tokens;

namespace Fluxkit.Server;

public static class AuthTokens
{
    public static object Create(FluxkitUser user, IConfiguration config)
    {
        var claims = new List<Claim> {
            new(JwtRegisteredClaimNames.Sub, user.Id.ToString()),
            new(JwtRegisteredClaimNames.Jti, Guid.NewGuid().ToString()),
            new("auth_mode", (config["Auth:Mode"] ?? "local").ToLowerInvariant()),
            new(ClaimTypes.Name, user.UserName ?? "") };
        if (user.IsServerAdmin) { claims.Add(new("role", "serverAdmin")); }
        var token = new JwtSecurityToken(config["Auth:Issuer"] ?? "fluxkit-self-hosted",
            config["Auth:Audience"] ?? "fluxkit-desktop", claims, DateTime.UtcNow,
            DateTime.UtcNow.AddHours(8), new SigningCredentials(
                new SymmetricSecurityKey(Encoding.UTF8.GetBytes(config["Auth:SigningKey"]!)),
                SecurityAlgorithms.HmacSha256));
        return new { accessToken = new JwtSecurityTokenHandler().WriteToken(token), expiresIn = 28800 };
    }
    public static ProjectRecord CreatePersonalProject(Guid ownerId, string username)
    {
        var id = Guid.NewGuid();
        var name = $"{username}'s personal project";
        return new ProjectRecord { Id = id, OwnerId = ownerId, Name = name, Kind = "personal", UpdatedBy = ownerId };
    }

}

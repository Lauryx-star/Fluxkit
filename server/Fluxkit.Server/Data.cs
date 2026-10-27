using Microsoft.AspNetCore.Identity;
using Microsoft.AspNetCore.Identity.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore;
using Microsoft.EntityFrameworkCore.Design;

namespace Fluxkit.Server;

public sealed class FluxkitUser : IdentityUser<Guid>
{
    public string Status { get; set; } = "pending";
    public bool IsServerAdmin { get; set; }
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}

public sealed class ProjectRecord
{
    public Guid Id { get; set; }
    public Guid OwnerId { get; set; }
    public string Name { get; set; } = "";
    public string Kind { get; set; } = "personal";
    public string Snapshot { get; set; } = "{}";
    public long Revision { get; set; } = 1;
    public string SharedValues { get; set; } = "[]";
    public long SharedValuesRevision { get; set; }
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}

public sealed class ProjectMembership
{
    public Guid ProjectId { get; set; }
    public Guid UserId { get; set; }
    public string Role { get; set; } = "member";
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}

public sealed class EncryptedSecretRecord
{
    public Guid ProjectId { get; set; }
    public Guid UserId { get; set; }
    public Guid VariableId { get; set; }
    public string EnvironmentId { get; set; } = "default";
    public string Salt { get; set; } = "";
    public string Nonce { get; set; } = "";
    public string Ciphertext { get; set; } = "";
    public string WrappedKey { get; set; } = "";
    public string WrappedKeyNonce { get; set; } = "";
    public string Algorithm { get; set; } = "AES-256-GCM";
    public string KdfParameters { get; set; } = "";
}

public sealed class AccountVault
{
    public Guid UserId { get; set; }
    public long Revision { get; set; } = 1;
    public string Profile { get; set; } = "{}";
}
public sealed class ProjectSecrets
{
    public Guid ProjectId { get; set; }
    public Guid UserId { get; set; }
    public long Revision { get; set; } = 1;
    public string Values { get; set; } = "[]";
}

public sealed class FluxkitDb(DbContextOptions<FluxkitDb> options)
    : IdentityDbContext<FluxkitUser, IdentityRole<Guid>, Guid>(options)
{
    public DbSet<AccountVault> AccountVaults => Set<AccountVault>();
    public DbSet<ProjectSecrets> ProjectSecrets => Set<ProjectSecrets>();
    public DbSet<ProjectRecord> Projects => Set<ProjectRecord>();
    public DbSet<ProjectMembership> Memberships => Set<ProjectMembership>();
    public DbSet<EncryptedSecretRecord> EncryptedSecrets => Set<EncryptedSecretRecord>();

    protected override void OnModelCreating(ModelBuilder builder)
    {
        base.OnModelCreating(builder);
        builder.Entity<FluxkitUser>().HasIndex(user => user.Status);
        builder.Entity<ProjectRecord>(entity =>
        {
            entity.HasKey(project => project.Id);
            entity.Property(project => project.Name).HasMaxLength(200).IsRequired();
            entity.Property(project => project.Kind).HasMaxLength(16).IsRequired();
            entity.Property(project => project.SharedValues).HasColumnType("jsonb").HasDefaultValue("[]");
            entity.Property(project => project.Snapshot).HasColumnType("jsonb").IsRequired();
            entity.HasIndex(project => project.OwnerId);
        });
        builder.Entity<AccountVault>(entity =>
        {
            entity.HasKey(vault => vault.UserId);
            entity.Property(vault => vault.Profile).HasColumnType("jsonb");
            entity.Property(vault => vault.Revision).IsConcurrencyToken();
            entity.HasOne<FluxkitUser>().WithMany().HasForeignKey(vault => vault.UserId).OnDelete(DeleteBehavior.Cascade);
        });
        builder.Entity<ProjectSecrets>(entity =>
        {
            entity.HasKey(values => new { values.ProjectId, values.UserId });
            entity.Property(values => values.Values).HasColumnType("jsonb");
            entity.Property(values => values.Revision).IsConcurrencyToken();
            entity.HasOne<ProjectRecord>().WithMany().HasForeignKey(values => values.ProjectId).OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<FluxkitUser>().WithMany().HasForeignKey(values => values.UserId).OnDelete(DeleteBehavior.Cascade);
        });
        builder.Entity<ProjectMembership>(entity =>
        {
            entity.HasKey(membership => new { membership.ProjectId, membership.UserId });
            entity.HasIndex(membership => membership.UserId);
            entity.Property(membership => membership.Role).HasMaxLength(32).IsRequired();
            entity.HasOne<ProjectRecord>().WithMany().HasForeignKey(membership => membership.ProjectId)
                .OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<FluxkitUser>().WithMany().HasForeignKey(membership => membership.UserId)
                .OnDelete(DeleteBehavior.Cascade);
        });
        builder.Entity<EncryptedSecretRecord>(entity =>
        {
            entity.HasKey(secret => new
            {
                secret.ProjectId,
                secret.UserId,
                secret.VariableId,
                secret.EnvironmentId
            });
            entity.Property(secret => secret.EnvironmentId).HasMaxLength(64);
            entity.Property(secret => secret.Salt).HasMaxLength(128);
            entity.Property(secret => secret.Nonce).HasMaxLength(64);
            entity.Property(secret => secret.Ciphertext).HasMaxLength(16_384);
            entity.Property(secret => secret.WrappedKey).HasMaxLength(16_384);
            entity.Property(secret => secret.WrappedKeyNonce).HasMaxLength(64);
            entity.Property(secret => secret.Algorithm).HasMaxLength(32);
            entity.Property(secret => secret.KdfParameters).HasMaxLength(1_024);
            entity.HasOne<ProjectRecord>().WithMany().HasForeignKey(secret => secret.ProjectId)
                .OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<FluxkitUser>().WithMany().HasForeignKey(secret => secret.UserId)
                .OnDelete(DeleteBehavior.Cascade);
        });
    }
}

public sealed class FluxkitDbFactory : IDesignTimeDbContextFactory<FluxkitDb>
{
    public FluxkitDb CreateDbContext(string[] args)
    {
        var connection = Environment.GetEnvironmentVariable("ConnectionStrings__Database")
            ?? "Host=localhost;Database=fluxkit;Username=fluxkit;Password=design-time-only";
        var options = new DbContextOptionsBuilder<FluxkitDb>().UseNpgsql(connection).Options;
        return new FluxkitDb(options);
    }
}

public sealed record RegisterInput(string Username, string Password);
public sealed record LoginInput(string Username, string Password);
public sealed record TakeoverInput(
    string Name, string Kind, string Snapshot, List<SecretEnvelope> EncryptedSecrets,
    List<SharedValue>? SharedValues = null, List<CipherValue>? VaultValues = null);
public sealed record UpdateProjectInput(string Snapshot);
public sealed record CreateTeamProjectInput(string Name);
public sealed record MembershipInput(string Username, string Role);
public sealed record UserStatusInput(string Status);
public sealed record SecretEnvelope(
    Guid VariableId, string EnvironmentId, string Salt, string Nonce, string Ciphertext,
    string WrappedKey, string WrappedKeyNonce, string Algorithm, string KdfParameters);

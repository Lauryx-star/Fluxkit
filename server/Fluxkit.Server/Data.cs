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
    /// Project-level authentication as the desktop model's JSON, or null.
    public string? Auth { get; set; }
    public long Revision { get; set; } = 1;
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
    public Guid? UpdatedBy { get; set; }
}

public sealed class ProjectMembership
{
    public Guid ProjectId { get; set; }
    public Guid UserId { get; set; }
    public string Role { get; set; } = "member";
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}

/// Columns every versioned project entity shares. `Revision` is the optimistic-concurrency
/// token checked by each change; `Position` orders siblings as a sortable rank string.
public abstract class ProjectEntity
{
    public Guid Id { get; set; }
    public Guid ProjectId { get; set; }
    public string Position { get; set; } = "";
    public long Revision { get; set; } = 1;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
    public Guid? UpdatedBy { get; set; }
}

public sealed class CollectionRecord : ProjectEntity
{
    public string Name { get; set; } = "";
    public string? Auth { get; set; }
    public string? OpenApi { get; set; }
    public string? Postman { get; set; }
}

public sealed class FolderRecord : ProjectEntity
{
    public Guid CollectionId { get; set; }
    public Guid? ParentId { get; set; }
    public string Name { get; set; } = "";
    public string? Auth { get; set; }
}

public sealed class RequestRecord : ProjectEntity
{
    public Guid CollectionId { get; set; }
    public Guid? FolderId { get; set; }
    public string Name { get; set; } = "";
    public string Method { get; set; } = "GET";
    public string Url { get; set; } = "";
    /// Params, headers, body, auth and import notes as the desktop model's JSON.
    public string Details { get; set; } = "{}";
}

public sealed class EnvironmentRecord : ProjectEntity
{
    public string Name { get; set; } = "";
}

public sealed class VariableRecord : ProjectEntity
{
    public string Name { get; set; } = "";
    public bool Secret { get; set; } = true;
}

/// One shared (non-secret) value slot. `EnvironmentId` is an environment UUID or `default`.
public sealed class SharedValueRecord
{
    public Guid ProjectId { get; set; }
    public Guid VariableId { get; set; }
    public string EnvironmentId { get; set; } = "default";
    public string Value { get; set; } = "";
    public long Revision { get; set; } = 1;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
    public Guid? UpdatedBy { get; set; }
}

/// One client-encrypted secret value slot of one user.
public sealed class VaultValueRecord
{
    public Guid ProjectId { get; set; }
    public Guid UserId { get; set; }
    public Guid VariableId { get; set; }
    public string EnvironmentId { get; set; } = "default";
    public string Nonce { get; set; } = "";
    public string Ciphertext { get; set; } = "";
    public long Revision { get; set; } = 1;
    public DateTimeOffset UpdatedAt { get; set; } = DateTimeOffset.UtcNow;
}

/// Append-only change feed. Clients poll it by `Seq` to learn about other users' edits.
public sealed class ProjectChangeRecord
{
    public long Seq { get; set; }
    public Guid ProjectId { get; set; }
    public string EntityKind { get; set; } = "";
    public Guid EntityId { get; set; }
    public string Operation { get; set; } = "put";
    public long Revision { get; set; }
    public Guid? ChangedBy { get; set; }
    public DateTimeOffset ChangedAt { get; set; } = DateTimeOffset.UtcNow;
}

public sealed class AccountVault
{
    public Guid UserId { get; set; }
    public long Revision { get; set; } = 1;
    public string Profile { get; set; } = "{}";
}

public sealed class FluxkitDb(DbContextOptions<FluxkitDb> options)
    : IdentityDbContext<FluxkitUser, IdentityRole<Guid>, Guid>(options)
{
    public DbSet<AccountVault> AccountVaults => Set<AccountVault>();
    public DbSet<ProjectRecord> Projects => Set<ProjectRecord>();
    public DbSet<ProjectMembership> Memberships => Set<ProjectMembership>();
    public DbSet<CollectionRecord> Collections => Set<CollectionRecord>();
    public DbSet<FolderRecord> Folders => Set<FolderRecord>();
    public DbSet<RequestRecord> Requests => Set<RequestRecord>();
    public DbSet<EnvironmentRecord> Environments => Set<EnvironmentRecord>();
    public DbSet<VariableRecord> Variables => Set<VariableRecord>();
    public DbSet<SharedValueRecord> SharedValues => Set<SharedValueRecord>();
    public DbSet<VaultValueRecord> VaultValues => Set<VaultValueRecord>();
    public DbSet<ProjectChangeRecord> ProjectChanges => Set<ProjectChangeRecord>();

    protected override void OnModelCreating(ModelBuilder builder)
    {
        base.OnModelCreating(builder);
        builder.Entity<FluxkitUser>().HasIndex(user => user.Status);
        builder.Entity<ProjectRecord>(entity =>
        {
            entity.HasKey(project => project.Id);
            entity.Property(project => project.Name).HasMaxLength(200).IsRequired();
            entity.Property(project => project.Kind).HasMaxLength(16).IsRequired();
            entity.Property(project => project.Auth).HasColumnType("jsonb");
            entity.Property(project => project.Revision).IsConcurrencyToken();
            entity.HasIndex(project => project.OwnerId);
        });
        builder.Entity<AccountVault>(entity =>
        {
            entity.HasKey(vault => vault.UserId);
            entity.Property(vault => vault.Profile).HasColumnType("jsonb");
            entity.Property(vault => vault.Revision).IsConcurrencyToken();
            entity.HasOne<FluxkitUser>().WithMany().HasForeignKey(vault => vault.UserId).OnDelete(DeleteBehavior.Cascade);
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
        builder.Entity<CollectionRecord>(entity =>
        {
            Versioned(entity);
            entity.ToTable("Collections");
            entity.Property(collection => collection.Name).HasMaxLength(200).IsRequired();
            entity.Property(collection => collection.Auth).HasColumnType("jsonb");
            entity.Property(collection => collection.OpenApi).HasColumnType("jsonb");
            entity.Property(collection => collection.Postman).HasColumnType("jsonb");
        });
        builder.Entity<FolderRecord>(entity =>
        {
            Versioned(entity);
            entity.ToTable("Folders");
            entity.Property(folder => folder.Name).HasMaxLength(200).IsRequired();
            entity.Property(folder => folder.Auth).HasColumnType("jsonb");
            entity.HasIndex(folder => folder.CollectionId);
            entity.HasOne<CollectionRecord>().WithMany().HasForeignKey(folder => folder.CollectionId)
                .OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<FolderRecord>().WithMany().HasForeignKey(folder => folder.ParentId)
                .OnDelete(DeleteBehavior.Cascade);
        });
        builder.Entity<RequestRecord>(entity =>
        {
            Versioned(entity);
            entity.ToTable("Requests");
            entity.Property(request => request.Name).HasMaxLength(200).IsRequired();
            entity.Property(request => request.Method).HasMaxLength(32).IsRequired();
            entity.Property(request => request.Url).IsRequired();
            entity.Property(request => request.Details).HasColumnType("jsonb").IsRequired();
            entity.HasIndex(request => request.CollectionId);
            entity.HasIndex(request => request.FolderId);
            entity.HasOne<CollectionRecord>().WithMany().HasForeignKey(request => request.CollectionId)
                .OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<FolderRecord>().WithMany().HasForeignKey(request => request.FolderId)
                .OnDelete(DeleteBehavior.Cascade);
        });
        builder.Entity<EnvironmentRecord>(entity =>
        {
            Versioned(entity);
            entity.ToTable("Environments");
            entity.Property(environment => environment.Name).HasMaxLength(200).IsRequired();
            entity.HasIndex(environment => new { environment.ProjectId, environment.Name }).IsUnique();
        });
        builder.Entity<VariableRecord>(entity =>
        {
            Versioned(entity);
            entity.ToTable("Variables");
            entity.Property(variable => variable.Name).HasMaxLength(200).IsRequired();
            entity.HasIndex(variable => new { variable.ProjectId, variable.Name }).IsUnique();
        });
        builder.Entity<SharedValueRecord>(entity =>
        {
            entity.ToTable("SharedValues");
            entity.HasKey(value => new { value.VariableId, value.EnvironmentId });
            entity.Property(value => value.EnvironmentId).HasMaxLength(64);
            entity.Property(value => value.Value).HasMaxLength(2560).IsRequired();
            entity.Property(value => value.Revision).IsConcurrencyToken();
            entity.HasIndex(value => value.ProjectId);
            entity.HasOne<ProjectRecord>().WithMany().HasForeignKey(value => value.ProjectId).OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<VariableRecord>().WithMany().HasForeignKey(value => value.VariableId).OnDelete(DeleteBehavior.Cascade);
        });
        builder.Entity<VaultValueRecord>(entity =>
        {
            entity.ToTable("VaultValues");
            entity.HasKey(value => new { value.UserId, value.VariableId, value.EnvironmentId });
            entity.Property(value => value.EnvironmentId).HasMaxLength(64);
            entity.Property(value => value.Nonce).HasMaxLength(64).IsRequired();
            entity.Property(value => value.Ciphertext).HasMaxLength(4096).IsRequired();
            entity.Property(value => value.Revision).IsConcurrencyToken();
            entity.HasIndex(value => new { value.ProjectId, value.UserId });
            entity.HasOne<ProjectRecord>().WithMany().HasForeignKey(value => value.ProjectId).OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<VariableRecord>().WithMany().HasForeignKey(value => value.VariableId).OnDelete(DeleteBehavior.Cascade);
            entity.HasOne<FluxkitUser>().WithMany().HasForeignKey(value => value.UserId).OnDelete(DeleteBehavior.Cascade);
        });
        builder.Entity<ProjectChangeRecord>(entity =>
        {
            entity.ToTable("ProjectChanges");
            entity.HasKey(change => change.Seq);
            entity.Property(change => change.Seq).UseIdentityAlwaysColumn();
            entity.Property(change => change.EntityKind).HasMaxLength(16).IsRequired();
            entity.Property(change => change.Operation).HasMaxLength(8).IsRequired();
            entity.HasIndex(change => new { change.ProjectId, change.Seq });
            entity.HasOne<ProjectRecord>().WithMany().HasForeignKey(change => change.ProjectId).OnDelete(DeleteBehavior.Cascade);
        });
    }

    private static void Versioned<T>(Microsoft.EntityFrameworkCore.Metadata.Builders.EntityTypeBuilder<T> entity)
        where T : ProjectEntity
    {
        entity.HasKey(item => item.Id);
        entity.Property(item => item.Position).HasMaxLength(128).IsRequired();
        entity.Property(item => item.Revision).IsConcurrencyToken();
        entity.HasIndex(item => item.ProjectId);
        entity.HasOne<ProjectRecord>().WithMany().HasForeignKey(item => item.ProjectId).OnDelete(DeleteBehavior.Cascade);
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
public sealed record CreateTeamProjectInput(string Name);
public sealed record MembershipInput(string Username, string Role);
public sealed record UserStatusInput(string Status);

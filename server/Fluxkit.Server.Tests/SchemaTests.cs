using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.DependencyInjection;
using Xunit;

namespace Fluxkit.Server.Tests;

/// The normalized schema on a real PostgreSQL database: migrations from an empty database,
/// cascading deletes and the uniqueness rules the API relies on.
public sealed class SchemaTests(ServerFixture server) : IClassFixture<ServerFixture>
{
    private sealed record Tree(Guid Project, Guid Collection, Guid Root, Guid Child, Guid Request,
        Guid NestedRequest, Guid Environment, Guid Variable, Guid User);

    private async Task<Tree> Seed(FluxkitDb db)
    {
        var user = await db.Users.Select(item => item.Id).FirstAsync();
        var tree = new Tree(Guid.NewGuid(), Guid.NewGuid(), Guid.NewGuid(), Guid.NewGuid(), Guid.NewGuid(),
            Guid.NewGuid(), Guid.NewGuid(), Guid.NewGuid(), user);
        db.Projects.Add(new ProjectRecord { Id = tree.Project, OwnerId = user, Name = "Schema", Kind = "personal" });
        db.Collections.Add(new CollectionRecord { Id = tree.Collection, ProjectId = tree.Project, Name = "API", Position = "a" });
        db.Folders.Add(new FolderRecord { Id = tree.Root, ProjectId = tree.Project, CollectionId = tree.Collection, Name = "Root", Position = "a" });
        db.Folders.Add(new FolderRecord { Id = tree.Child, ProjectId = tree.Project, CollectionId = tree.Collection, ParentId = tree.Root, Name = "Child", Position = "a" });
        db.Requests.Add(new RequestRecord { Id = tree.Request, ProjectId = tree.Project, CollectionId = tree.Collection, Name = "Top", Url = "https://synthetic.invalid", Position = "a" });
        db.Requests.Add(new RequestRecord { Id = tree.NestedRequest, ProjectId = tree.Project, CollectionId = tree.Collection, FolderId = tree.Child, Name = "Nested", Url = "https://synthetic.invalid/nested", Position = "a" });
        db.Environments.Add(new EnvironmentRecord { Id = tree.Environment, ProjectId = tree.Project, Name = "Test", Position = "a" });
        db.Variables.Add(new VariableRecord { Id = tree.Variable, ProjectId = tree.Project, Name = "baseUrl", Secret = false, Position = "a" });
        db.SharedValues.Add(new SharedValueRecord { ProjectId = tree.Project, VariableId = tree.Variable, EnvironmentId = tree.Environment.ToString(), Value = "https://synthetic.invalid" });
        db.VaultValues.Add(new VaultValueRecord { ProjectId = tree.Project, UserId = user, VariableId = tree.Variable, Nonce = "bm9uY2U=", Ciphertext = "Y2lwaGVy" });
        db.ProjectChanges.Add(new ProjectChangeRecord { ProjectId = tree.Project, EntityKind = "request", EntityId = tree.Request, Revision = 1 });
        await db.SaveChangesAsync();
        db.ChangeTracker.Clear();
        return tree;
    }

    private FluxkitDb Db(IServiceScope scope) => scope.ServiceProvider.GetRequiredService<FluxkitDb>();

    [Fact]
    public async Task Migrations_create_the_normalized_tables_on_an_empty_database()
    {
        using var scope = server.Services.CreateScope();
        var db = Db(scope);
        Assert.Empty(await db.Database.GetPendingMigrationsAsync());
        var tables = await db.Database.SqlQueryRaw<string>(
            "SELECT table_name AS \"Value\" FROM information_schema.tables WHERE table_schema = 'public'").ToListAsync();
        foreach (var table in new[] { "Projects", "Collections", "Folders", "Requests", "Environments", "Variables", "SharedValues", "VaultValues", "ProjectChanges" })
        {
            Assert.Contains(table, tables);
        }
        Assert.DoesNotContain("ProjectSecrets", tables);
        Assert.DoesNotContain("EncryptedSecrets", tables);
        var columns = await db.Database.SqlQueryRaw<string>(
            "SELECT column_name AS \"Value\" FROM information_schema.columns WHERE table_name = 'Projects'").ToListAsync();
        Assert.DoesNotContain("Snapshot", columns);
        Assert.DoesNotContain("SharedValues", columns);
    }

    [Fact]
    public async Task Deleting_a_folder_removes_its_subfolders_and_their_requests_only()
    {
        using var scope = server.Services.CreateScope();
        var db = Db(scope);
        var tree = await Seed(db);
        await db.Folders.Where(folder => folder.Id == tree.Root).ExecuteDeleteAsync();
        Assert.False(await db.Folders.AnyAsync(folder => folder.Id == tree.Child));
        Assert.False(await db.Requests.AnyAsync(request => request.Id == tree.NestedRequest));
        Assert.True(await db.Requests.AnyAsync(request => request.Id == tree.Request));
        Assert.True(await db.Collections.AnyAsync(collection => collection.Id == tree.Collection));
    }

    [Fact]
    public async Task Deleting_a_variable_removes_its_shared_and_vault_values()
    {
        using var scope = server.Services.CreateScope();
        var db = Db(scope);
        var tree = await Seed(db);
        await db.Variables.Where(variable => variable.Id == tree.Variable).ExecuteDeleteAsync();
        Assert.False(await db.SharedValues.AnyAsync(value => value.VariableId == tree.Variable));
        Assert.False(await db.VaultValues.AnyAsync(value => value.VariableId == tree.Variable));
        Assert.True(await db.Environments.AnyAsync(environment => environment.Id == tree.Environment));
    }

    [Fact]
    public async Task Deleting_a_project_removes_every_row_that_belongs_to_it()
    {
        using var scope = server.Services.CreateScope();
        var db = Db(scope);
        var tree = await Seed(db);
        await db.Projects.Where(project => project.Id == tree.Project).ExecuteDeleteAsync();
        Assert.False(await db.Collections.AnyAsync(item => item.ProjectId == tree.Project));
        Assert.False(await db.Folders.AnyAsync(item => item.ProjectId == tree.Project));
        Assert.False(await db.Requests.AnyAsync(item => item.ProjectId == tree.Project));
        Assert.False(await db.Environments.AnyAsync(item => item.ProjectId == tree.Project));
        Assert.False(await db.Variables.AnyAsync(item => item.ProjectId == tree.Project));
        Assert.False(await db.SharedValues.AnyAsync(item => item.ProjectId == tree.Project));
        Assert.False(await db.VaultValues.AnyAsync(item => item.ProjectId == tree.Project));
        Assert.False(await db.ProjectChanges.AnyAsync(item => item.ProjectId == tree.Project));
    }

    [Fact]
    public async Task Variable_and_environment_names_are_unique_per_project_only()
    {
        using var scope = server.Services.CreateScope();
        var db = Db(scope);
        var tree = await Seed(db);
        db.Variables.Add(new VariableRecord { Id = Guid.NewGuid(), ProjectId = tree.Project, Name = "baseUrl", Position = "b" });
        await Assert.ThrowsAsync<DbUpdateException>(() => db.SaveChangesAsync());
        db.ChangeTracker.Clear();
        db.Environments.Add(new EnvironmentRecord { Id = Guid.NewGuid(), ProjectId = tree.Project, Name = "Test", Position = "b" });
        await Assert.ThrowsAsync<DbUpdateException>(() => db.SaveChangesAsync());
        db.ChangeTracker.Clear();
        var other = await Seed(db);
        Assert.True(await db.Variables.AnyAsync(variable => variable.ProjectId == other.Project && variable.Name == "baseUrl"));
    }

    [Fact]
    public async Task Change_feed_sequence_numbers_increase_monotonically()
    {
        using var scope = server.Services.CreateScope();
        var db = Db(scope);
        var tree = await Seed(db);
        for (var i = 0; i < 3; i++)
        {
            db.ProjectChanges.Add(new ProjectChangeRecord { ProjectId = tree.Project, EntityKind = "request", EntityId = tree.Request, Revision = i + 2 });
            await db.SaveChangesAsync();
        }
        var seqs = await db.ProjectChanges.Where(change => change.ProjectId == tree.Project).OrderBy(change => change.Seq).Select(change => change.Seq).ToListAsync();
        Assert.Equal(4, seqs.Count);
        Assert.Equal(seqs.OrderBy(seq => seq), seqs);
        Assert.Equal(seqs.Count, seqs.Distinct().Count());
    }
}

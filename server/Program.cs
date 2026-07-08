using System.ComponentModel.DataAnnotations;
using System.IdentityModel.Tokens.Jwt;
using System.Security.Claims;
using System.Security.Cryptography;
using System.Text;
using System.Text.Json;
using System.Text.Json.Serialization;
using Microsoft.AspNetCore.Authentication.JwtBearer;
using Microsoft.AspNetCore.Http.HttpResults;
using Microsoft.AspNetCore.Mvc;
using Microsoft.EntityFrameworkCore;
using Microsoft.Extensions.Options;
using Microsoft.IdentityModel.Tokens;
using Org.BouncyCastle.Crypto.Parameters;
using Org.BouncyCastle.Crypto.Signers;
using Org.BouncyCastle.Security;

var builder = WebApplication.CreateBuilder(args);

builder.Logging.ClearProviders();
builder.Logging.AddConsole();

builder.Services.AddOptions<MtyOptions>()
    .Bind(builder.Configuration.GetSection("Mty"))
    .ValidateDataAnnotations()
    .ValidateOnStart();

builder.Services.AddDbContext<MtyDbContext>(options =>
    options.UseNpgsql(builder.Configuration.GetConnectionString("Postgres")));

builder.Services.AddScoped<PackageStorage>();
builder.Services.AddSingleton<PackageSigner>();
builder.Services.AddScoped<AuthService>();
builder.Services.AddProblemDetails();
builder.Services.AddOpenApi();
builder.Services.AddHealthChecks();
builder.Services.ConfigureHttpJsonOptions(options =>
{
    options.SerializerOptions.PropertyNamingPolicy = JsonNamingPolicy.CamelCase;
    options.SerializerOptions.Converters.Add(new JsonStringEnumConverter(JsonNamingPolicy.CamelCase));
});
builder.Services.AddCors(options =>
{
    options.AddPolicy("admin-web", policy =>
        policy.AllowAnyHeader().AllowAnyMethod().AllowAnyOrigin());
});

var jwtOptions = builder.Configuration.GetSection("Mty:Jwt").Get<JwtOptions>() ?? new JwtOptions();
var jwtKey = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(jwtOptions.SigningKey));

builder.Services.AddAuthentication(JwtBearerDefaults.AuthenticationScheme)
    .AddJwtBearer(options =>
    {
        options.TokenValidationParameters = new TokenValidationParameters
        {
            ValidateIssuer = true,
            ValidateAudience = true,
            ValidateIssuerSigningKey = true,
            ValidateLifetime = true,
            ValidIssuer = jwtOptions.Issuer,
            ValidAudience = jwtOptions.Audience,
            IssuerSigningKey = jwtKey
        };
    });
builder.Services.AddAuthorization();

var app = builder.Build();

app.UseExceptionHandler();
app.UseCors("admin-web");
app.UseAuthentication();
app.UseAuthorization();

if (app.Environment.IsDevelopment())
{
    app.MapOpenApi();
    app.MapGet("/api/admin/routes", (IEnumerable<EndpointDataSource> sources) =>
        sources.SelectMany(s => s.Endpoints)
            .OfType<RouteEndpoint>()
            .Select(e => new
            {
                route = e.RoutePattern.RawText,
                methods = e.Metadata.OfType<HttpMethodMetadata>().FirstOrDefault()?.HttpMethods ?? []
            })
            .OrderBy(e => e.route));
}

app.MapHealthChecks("/health");

app.MapGet("/api/signing-key", (PackageSigner signer) =>
    TypedResults.Ok(new SigningKeyDto("Ed25519", signer.PublicKey)));

using (var scope = app.Services.CreateScope())
{
    var db = scope.ServiceProvider.GetRequiredService<MtyDbContext>();
    await db.Database.EnsureCreatedAsync();
    await SeedAdminAsync(db, scope.ServiceProvider.GetRequiredService<IOptions<MtyOptions>>().Value.Admin);
}

app.MapPost("/api/auth/login", async Task<Results<Ok<LoginResponse>, UnauthorizedHttpResult>> (
    LoginRequest request,
    AuthService auth,
    MtyDbContext db,
    IOptions<MtyOptions> options) =>
{
    var user = await db.Users.SingleOrDefaultAsync(u => u.Username == request.Username && u.IsActive);
    if (user is null || !PasswordHasher.Verify(request.Password, user.PasswordHash))
    {
        return TypedResults.Unauthorized();
    }

    var token = auth.CreateToken(user);
    await AddAuditAsync(db, user.Username, "login", "auth", user.Username);
    return TypedResults.Ok(new LoginResponse(token, user.Username));
});

app.MapGet("/api/packages", async ([FromQuery] string? keyword, [FromQuery] string? platform, [FromQuery] string? arch, MtyDbContext db) =>
{
    var query = db.Packages.AsNoTracking().Include(p => p.Versions).AsQueryable();
    if (!string.IsNullOrWhiteSpace(keyword))
    {
        query = query.Where(p => p.Name.Contains(keyword) || p.Description.Contains(keyword));
    }

    var packages = await query
        .OrderBy(p => p.Name)
        .Select(p => new PackageSummaryDto(
            p.Name,
            p.Description,
            p.Versions
                .Where(v => v.Status == PackageVersionStatus.Published &&
                    (platform == null || v.Platform == platform) &&
                    (arch == null || v.Arch == arch))
                .OrderByDescending(v => v.CreatedAt)
                .Select(v => v.Version)
                .FirstOrDefault()))
        .ToListAsync();

    return TypedResults.Ok(packages);
});

app.MapGet("/api/packages/{name}", async Task<Results<Ok<PackageDetailDto>, NotFound>> (string name, MtyDbContext db) =>
{
    var package = await db.Packages.AsNoTracking()
        .Include(p => p.Versions)
        .SingleOrDefaultAsync(p => p.Name == name);
    if (package is null)
    {
        return TypedResults.NotFound();
    }

    var versions = package.Versions
        .Where(v => v.Status == PackageVersionStatus.Published)
        .OrderByDescending(v => v.CreatedAt)
        .Select(v => ToVersionDto(package.Name, v))
        .ToList();
    return TypedResults.Ok(new PackageDetailDto(package.Name, package.Description, versions));
});

app.MapGet("/api/packages/{name}/versions", async (string name, MtyDbContext db) =>
{
    var versions = await db.PackageVersions.AsNoTracking()
        .Where(v => v.Package.Name == name && v.Status == PackageVersionStatus.Published)
        .OrderByDescending(v => v.CreatedAt)
        .Select(v => new PackageVersionDto(
            v.Version,
            v.Platform,
            v.Arch,
            v.Sha256,
            v.Signature,
            $"/api/packages/{name}/versions/{v.Version}/download?platform={Uri.EscapeDataString(v.Platform)}&arch={Uri.EscapeDataString(v.Arch)}"))
        .ToListAsync();
    return TypedResults.Ok(versions);
});

app.MapGet("/api/packages/{name}/versions/{version}/manifest", async Task<IResult> (
    string name,
    string version,
    [FromQuery] string? platform,
    [FromQuery] string? arch,
    MtyDbContext db,
    PackageStorage storage) =>
{
    var packageVersion = await FindPublishedVersionAsync(db, name, version, platform, arch);
    if (packageVersion.Status == VersionLookupStatus.NotFound)
    {
        return Results.NotFound();
    }
    if (packageVersion.Status == VersionLookupStatus.Ambiguous)
    {
        return AmbiguousVersionResult(name, version);
    }

    return Results.File(Encoding.UTF8.GetBytes(packageVersion.Entity!.ManifestJson), "application/json", "manifest.json");
});

app.MapGet("/api/packages/{name}/versions/{version}/download", async Task<IResult> (
    string name,
    string version,
    [FromQuery] string? platform,
    [FromQuery] string? arch,
    MtyDbContext db,
    PackageStorage storage) =>
{
    var packageVersion = await FindPublishedVersionAsync(db, name, version, platform, arch);
    if (packageVersion.Status == VersionLookupStatus.NotFound)
    {
        return Results.NotFound();
    }
    if (packageVersion.Status == VersionLookupStatus.Ambiguous)
    {
        return AmbiguousVersionResult(name, version);
    }

    var entity = packageVersion.Entity!;
    if (!storage.Exists(entity.FilePath))
    {
        return Results.NotFound();
    }

    entity.DownloadCount++;
    await db.SaveChangesAsync();
    return TypedResults.PhysicalFile(storage.Resolve(entity.FilePath), "application/octet-stream", $"{name}-{version}.mty");
});

app.MapGet("/api/self-update", async Task<Results<Ok<PackageVersionDto>, NotFound>> (
    [FromQuery] string platform,
    [FromQuery] string arch,
    MtyDbContext db) =>
{
    var version = await FindLatestPublishedVersionAsync(db, "mty", platform, arch);
    return version is null
        ? TypedResults.NotFound()
        : TypedResults.Ok(ToVersionDto("mty", version));
});

app.MapGet("/api/self-update/download", async Task<Results<PhysicalFileHttpResult, NotFound>> (
    [FromQuery] string platform,
    [FromQuery] string arch,
    MtyDbContext db,
    PackageStorage storage) =>
{
    var version = await FindLatestPublishedVersionAsync(db, "mty", platform, arch);
    if (version is null || !storage.Exists(version.FilePath))
    {
        return TypedResults.NotFound();
    }

    version.DownloadCount++;
    await db.SaveChangesAsync();
    return TypedResults.PhysicalFile(storage.Resolve(version.FilePath), "application/octet-stream", $"mty-{version.Version}.mty");
});

var admin = app.MapGroup("/api/admin").RequireAuthorization();

admin.MapGet("/packages", async (MtyDbContext db) =>
{
    var packages = await db.Packages.AsNoTracking()
        .Include(p => p.Versions)
        .OrderBy(p => p.Name)
        .Select(p => new AdminPackageDto(
            p.Id,
            p.Name,
            p.Description,
            p.Versions.Count,
            p.Versions.OrderByDescending(v => v.CreatedAt).Select(v => v.Version).FirstOrDefault()))
        .ToListAsync();
    return TypedResults.Ok(packages);
});

admin.MapGet("/packages/{name}", async Task<Results<Ok<AdminPackageDetailDto>, NotFound>> (string name, MtyDbContext db) =>
{
    var package = await db.Packages.AsNoTracking()
        .Include(p => p.Versions)
        .SingleOrDefaultAsync(p => p.Name == name);
    if (package is null)
    {
        return TypedResults.NotFound();
    }

    var versions = package.Versions
        .OrderByDescending(v => v.CreatedAt)
        .Select(v => new AdminPackageVersionDto(
            v.Version,
            v.Platform,
            v.Arch,
            v.Sha256,
            v.Signature,
            v.Status.ToString(),
            v.DownloadCount,
            v.CreatedAt))
        .ToList();
    return TypedResults.Ok(new AdminPackageDetailDto(package.Name, package.Description, versions));
});

admin.MapPost("/packages", async (CreatePackageRequest request, ClaimsPrincipal principal, MtyDbContext db) =>
{
    if (await db.Packages.AnyAsync(p => p.Name == request.Name))
    {
        return Results.Problem(statusCode: StatusCodes.Status409Conflict, title: "Package already exists");
    }

    var package = new PackageEntity { Name = request.Name, Description = request.Description };
    db.Packages.Add(package);
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "create-package", "package", request.Name);
    await db.SaveChangesAsync();
    return Results.Created($"/api/packages/{request.Name}", new { package.Id, package.Name, package.Description });
});

admin.MapPut("/packages/{name}", async (string name, UpdatePackageRequest request, ClaimsPrincipal principal, MtyDbContext db) =>
{
    var package = await db.Packages.SingleOrDefaultAsync(p => p.Name == name);
    if (package is null)
    {
        return Results.NotFound();
    }

    if (!string.Equals(name, request.Name, StringComparison.OrdinalIgnoreCase) &&
        await db.Packages.AnyAsync(p => p.Name == request.Name))
    {
        return Results.Problem(statusCode: StatusCodes.Status409Conflict, title: "Package already exists");
    }

    package.Name = request.Name;
    package.Description = request.Description;
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "update-package", "package", request.Name);
    await db.SaveChangesAsync();
    return Results.Ok(new { package.Id, package.Name, package.Description });
});

admin.MapDelete("/packages/{name}", async (string name, ClaimsPrincipal principal, MtyDbContext db, PackageStorage storage) =>
{
    var package = await db.Packages.Include(p => p.Versions).SingleOrDefaultAsync(p => p.Name == name);
    if (package is null)
    {
        return Results.NotFound();
    }

    storage.DeletePackage(name);
    foreach (var version in package.Versions)
    {
        storage.Delete(version.FilePath);
    }
    db.PackageVersions.RemoveRange(package.Versions);
    db.Packages.Remove(package);
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "delete-package", "package", name);
    await db.SaveChangesAsync();
    return Results.NoContent();
});

admin.MapPost("/packages/{name}/versions", async Task<IResult> (
    string name,
    [FromForm] UploadVersionRequest request,
    ClaimsPrincipal principal,
    MtyDbContext db,
    PackageStorage storage,
    PackageSigner signer) =>
{
    await using var stream = request.File.OpenReadStream();
    var staged = await storage.StageAsync(stream);
    string manifestJson;
    PackageManifestDto manifest;
    try
    {
        manifestJson = await storage.ReadManifestJsonFromFullPathAsync(staged.FullPath);
        manifest = ParseManifest(manifestJson);
    }
    catch (Exception ex) when (ex is InvalidDataException or JsonException or ValidationException)
    {
        storage.DeleteFullPath(staged.FullPath);
        return Results.Problem(statusCode: StatusCodes.Status400BadRequest, title: "Invalid .mty package", detail: ex.Message);
    }

    if (!string.Equals(manifest.Name, name, StringComparison.OrdinalIgnoreCase))
    {
        storage.DeleteFullPath(staged.FullPath);
        return Results.Problem(statusCode: StatusCodes.Status400BadRequest, title: "Manifest name does not match route package name");
    }

    var package = await db.Packages.Include(p => p.Versions).SingleOrDefaultAsync(p => p.Name == manifest.Name);
    if (package is null)
    {
        package = new PackageEntity
        {
            Name = manifest.Name,
            Description = manifest.Description ?? manifest.Name
        };
        db.Packages.Add(package);
        await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "create-package", "package", manifest.Name);
    }

    if (package.Versions.Any(v => v.Version == manifest.Version && v.Platform == manifest.Platform && v.Arch == manifest.Arch))
    {
        storage.DeleteFullPath(staged.FullPath);
        return Results.Problem(statusCode: StatusCodes.Status409Conflict, title: "Version already exists for platform and architecture");
    }

    var stored = await storage.CommitStagedAsync(staged, manifest.Name, manifest.Version, manifest.Platform, manifest.Arch);
    ValidateManifest(manifestJson, manifest.Name, manifest.Version, manifest.Platform, manifest.Arch);

    var version = new PackageVersionEntity
    {
        Package = package,
        Version = manifest.Version,
        Platform = manifest.Platform,
        Arch = manifest.Arch,
        Sha256 = stored.Sha256,
        Signature = signer.Sign(stored.Sha256),
        FilePath = stored.RelativePath,
        ManifestJson = manifestJson,
        Status = PackageVersionStatus.Draft
    };
    db.PackageVersions.Add(version);
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "upload-version", "package-version", $"{manifest.Name}@{manifest.Version}");
    await db.SaveChangesAsync();
    return Results.Created($"/api/packages/{manifest.Name}/versions/{manifest.Version}", ToVersionDto(manifest.Name, version));
}).DisableAntiforgery();

admin.MapPost("/packages/{name}/versions/from-executable", async Task<IResult> (
    string name,
    [FromForm] GenerateVersionRequest request,
    ClaimsPrincipal principal,
    MtyDbContext db,
    PackageStorage storage,
    PackageSigner signer) =>
{
    if (request.File.Length == 0)
    {
        return Results.Problem(statusCode: StatusCodes.Status400BadRequest, title: "Executable file is required");
    }

    var entryFileName = SanitizeFileName(request.File.FileName);
    var entryPath = $"bin/{entryFileName}";
    var executableBytes = await ReadAllBytesAsync(request.File);
    var manifest = new GeneratedPackageManifest(
        name,
        request.Version,
        string.IsNullOrWhiteSpace(request.Description) ? name : request.Description,
        request.Platform,
        request.Arch,
        entryPath,
        [],
        [new GeneratedManifestFile(entryPath, Sha256Hex(executableBytes), true)]);

    var manifestJson = JsonSerializer.Serialize(manifest, JsonDefaults.Options);
    await using var packageStream = BuildMtyPackage(manifestJson, entryPath, executableBytes);
    var staged = await storage.StageAsync(packageStream);

    var package = await db.Packages.Include(p => p.Versions).SingleOrDefaultAsync(p => p.Name == name);
    if (package is null)
    {
        package = new PackageEntity
        {
            Name = name,
            Description = manifest.Description
        };
        db.Packages.Add(package);
        await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "create-package", "package", name);
    }

    if (package.Versions.Any(v => v.Version == request.Version && v.Platform == request.Platform && v.Arch == request.Arch))
    {
        storage.DeleteFullPath(staged.FullPath);
        return Results.Problem(statusCode: StatusCodes.Status409Conflict, title: "Version already exists for platform and architecture");
    }

    var stored = await storage.CommitStagedAsync(staged, name, request.Version, request.Platform, request.Arch);
    var version = new PackageVersionEntity
    {
        Package = package,
        Version = request.Version,
        Platform = request.Platform,
        Arch = request.Arch,
        Sha256 = stored.Sha256,
        Signature = signer.Sign(stored.Sha256),
        FilePath = stored.RelativePath,
        ManifestJson = manifestJson,
        Status = PackageVersionStatus.Draft
    };
    db.PackageVersions.Add(version);
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "generate-version", "package-version", $"{name}@{request.Version}");
    await db.SaveChangesAsync();
    return Results.Created($"/api/packages/{name}/versions/{request.Version}", ToVersionDto(name, version));
}).DisableAntiforgery();

admin.MapPost("/packages/{name}/versions/{version}/publish", async (
    string name,
    string version,
    [FromQuery] string? platform,
    [FromQuery] string? arch,
    ClaimsPrincipal principal,
    MtyDbContext db) =>
    await SetStatusAsync(db, principal, name, version, platform, arch, PackageVersionStatus.Published, "publish-version"));

admin.MapPost("/packages/{name}/versions/{version}/unpublish", async (
    string name,
    string version,
    [FromQuery] string? platform,
    [FromQuery] string? arch,
    ClaimsPrincipal principal,
    MtyDbContext db) =>
    await SetStatusAsync(db, principal, name, version, platform, arch, PackageVersionStatus.Unpublished, "unpublish-version"));

admin.MapPost("/packages/{name}/versions/{version}/resign", async (
    string name,
    string version,
    [FromQuery] string? platform,
    [FromQuery] string? arch,
    ClaimsPrincipal principal,
    MtyDbContext db,
    PackageSigner signer) =>
{
    var lookup = await FindVersionAsync(db, name, version, platform, arch);
    if (lookup.Status == VersionLookupStatus.NotFound)
    {
        return Results.NotFound();
    }
    if (lookup.Status == VersionLookupStatus.Ambiguous)
    {
        return AmbiguousVersionResult(name, version);
    }

    var entity = lookup.Entity!;
    entity.Signature = signer.Sign(entity.Sha256);
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "resign-version", "package-version", $"{name}@{version}");
    await db.SaveChangesAsync();
    return Results.Ok(ToVersionDto(name, entity));
});

admin.MapGet("/packages/{name}/versions/{version}/download", async (
    string name,
    string version,
    [FromQuery] string? platform,
    [FromQuery] string? arch,
    MtyDbContext db,
    PackageStorage storage) =>
{
    var lookup = await FindVersionAsync(db, name, version, platform, arch);
    if (lookup.Status == VersionLookupStatus.NotFound)
    {
        return Results.NotFound();
    }
    if (lookup.Status == VersionLookupStatus.Ambiguous)
    {
        return AmbiguousVersionResult(name, version);
    }

    var entity = lookup.Entity!;
    if (!storage.Exists(entity.FilePath))
    {
        return Results.NotFound();
    }

    entity.DownloadCount++;
    await db.SaveChangesAsync();
    return TypedResults.PhysicalFile(
        storage.Resolve(entity.FilePath),
        "application/octet-stream",
        $"{name}-{version}-{entity.Platform}-{entity.Arch}.mty");
});

admin.MapDelete("/packages/{name}/versions/{version}", async (
    string name,
    string version,
    [FromQuery] string? platform,
    [FromQuery] string? arch,
    ClaimsPrincipal principal,
    MtyDbContext db,
    PackageStorage storage) =>
{
    var lookup = await FindVersionAsync(db, name, version, platform, arch);
    if (lookup.Status == VersionLookupStatus.NotFound)
    {
        return Results.NotFound();
    }
    if (lookup.Status == VersionLookupStatus.Ambiguous)
    {
        return AmbiguousVersionResult(name, version);
    }

    var entity = lookup.Entity!;
    storage.Delete(entity.FilePath);
    db.PackageVersions.Remove(entity);
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", "delete-version", "package-version", $"{name}@{version}");
    await db.SaveChangesAsync();
    return Results.NoContent();
});

admin.MapGet("/audit-logs", async (MtyDbContext db) =>
{
    var logs = await db.AuditLogs.AsNoTracking()
        .OrderByDescending(l => l.CreatedAt)
        .Take(200)
        .Select(l => new AuditLogDto(l.CreatedAt, l.Actor, l.Action, l.TargetType, l.Target))
        .ToListAsync();
    return TypedResults.Ok(logs);
});

app.Run();

static PackageVersionDto ToVersionDto(string packageName, PackageVersionEntity version) =>
    new(version.Version, version.Platform, version.Arch, version.Sha256, version.Signature,
        $"/api/packages/{packageName}/versions/{version.Version}/download?platform={Uri.EscapeDataString(version.Platform)}&arch={Uri.EscapeDataString(version.Arch)}");

static async Task<VersionLookupResult> FindPublishedVersionAsync(
    MtyDbContext db,
    string name,
    string version,
    string? platform,
    string? arch) =>
    await FindVersionAsync(db, name, version, platform, arch, PackageVersionStatus.Published);

static async Task<VersionLookupResult> FindVersionAsync(
    MtyDbContext db,
    string name,
    string version,
    string? platform,
    string? arch,
    PackageVersionStatus? status = null)
{
    var query = db.PackageVersions.Include(v => v.Package)
        .Where(v => v.Package.Name == name && v.Version == version);
    if (!string.IsNullOrWhiteSpace(platform))
    {
        query = query.Where(v => v.Platform == platform);
    }
    if (!string.IsNullOrWhiteSpace(arch))
    {
        query = query.Where(v => v.Arch == arch);
    }
    if (status is not null)
    {
        query = query.Where(v => v.Status == status);
    }

    var matches = await query.Take(2).ToListAsync();
    return matches.Count switch
    {
        0 => new VersionLookupResult(VersionLookupStatus.NotFound, null),
        1 => new VersionLookupResult(VersionLookupStatus.Found, matches[0]),
        _ => new VersionLookupResult(VersionLookupStatus.Ambiguous, null)
    };
}

static IResult AmbiguousVersionResult(string name, string version) =>
    Results.Problem(
        statusCode: StatusCodes.Status409Conflict,
        title: "Version target is ambiguous",
        detail: $"Package {name} version {version} exists for multiple platforms. Specify platform and arch.");

static async Task<PackageVersionEntity?> FindLatestPublishedVersionAsync(MtyDbContext db, string name, string platform, string arch) =>
    await db.PackageVersions.Include(v => v.Package)
        .Where(v => v.Package.Name == name && v.Platform == platform && v.Arch == arch && v.Status == PackageVersionStatus.Published)
        .OrderByDescending(v => v.CreatedAt)
        .FirstOrDefaultAsync();

static void ValidateManifest(string manifestJson, string name, string version, string platform, string arch)
{
    var manifest = ParseManifest(manifestJson);
    if (manifest.Name != name || manifest.Version != version || manifest.Platform != platform || manifest.Arch != arch)
    {
        throw new ValidationException("Manifest identity does not match upload metadata");
    }
}

static PackageManifestDto ParseManifest(string manifestJson) =>
    JsonSerializer.Deserialize<PackageManifestDto>(manifestJson, JsonDefaults.Options)
    ?? throw new ValidationException("Invalid manifest");

static async Task<byte[]> ReadAllBytesAsync(IFormFile file)
{
    await using var stream = file.OpenReadStream();
    using var memory = new MemoryStream();
    await stream.CopyToAsync(memory);
    return memory.ToArray();
}

static MemoryStream BuildMtyPackage(string manifestJson, string entryPath, byte[] executableBytes)
{
    var output = new MemoryStream();
    using (var archive = new System.IO.Compression.ZipArchive(output, System.IO.Compression.ZipArchiveMode.Create, leaveOpen: true))
    {
        var manifestEntry = archive.CreateEntry("manifest.json", System.IO.Compression.CompressionLevel.Optimal);
        using (var writer = new StreamWriter(manifestEntry.Open(), new UTF8Encoding(encoderShouldEmitUTF8Identifier: false)))
        {
            writer.Write(manifestJson);
        }

        var executableEntry = archive.CreateEntry(entryPath.Replace('\\', '/'), System.IO.Compression.CompressionLevel.Optimal);
        using var entryStream = executableEntry.Open();
        entryStream.Write(executableBytes);
    }

    output.Position = 0;
    return output;
}

static string Sha256Hex(byte[] bytes) => Convert.ToHexString(SHA256.HashData(bytes)).ToLowerInvariant();

static string SanitizeFileName(string fileName)
{
    var sanitized = Path.GetFileName(fileName);
    foreach (var invalid in Path.GetInvalidFileNameChars())
    {
        sanitized = sanitized.Replace(invalid, '_');
    }

    return string.IsNullOrWhiteSpace(sanitized) ? "tool" : sanitized;
}

static async Task<IResult> SetStatusAsync(
    MtyDbContext db,
    ClaimsPrincipal principal,
    string name,
    string version,
    string? platform,
    string? arch,
    PackageVersionStatus status,
    string action)
{
    var lookup = await FindVersionAsync(db, name, version, platform, arch);
    if (lookup.Status == VersionLookupStatus.NotFound)
    {
        return Results.NotFound();
    }
    if (lookup.Status == VersionLookupStatus.Ambiguous)
    {
        return AmbiguousVersionResult(name, version);
    }

    var entity = lookup.Entity!;
    entity.Status = status;
    await AddAuditAsync(db, principal.Identity?.Name ?? "admin", action, "package-version", $"{name}@{version}");
    await db.SaveChangesAsync();
    return Results.Ok(ToVersionDto(name, entity));
}

static async Task AddAuditAsync(MtyDbContext db, string actor, string action, string targetType, string target)
{
    db.AuditLogs.Add(new AuditLogEntity { Actor = actor, Action = action, TargetType = targetType, Target = target });
    await Task.CompletedTask;
}

static async Task SeedAdminAsync(MtyDbContext db, AdminOptions options)
{
    if (await db.Users.AnyAsync())
    {
        return;
    }

    db.Users.Add(new UserEntity
    {
        Username = options.Username,
        PasswordHash = PasswordHasher.Hash(options.Password),
        Role = "Admin"
    });
    await db.SaveChangesAsync();
}

sealed class MtyDbContext(DbContextOptions<MtyDbContext> options) : DbContext(options)
{
    public DbSet<PackageEntity> Packages => Set<PackageEntity>();
    public DbSet<PackageVersionEntity> PackageVersions => Set<PackageVersionEntity>();
    public DbSet<UserEntity> Users => Set<UserEntity>();
    public DbSet<AuditLogEntity> AuditLogs => Set<AuditLogEntity>();

    protected override void OnModelCreating(ModelBuilder modelBuilder)
    {
        modelBuilder.Entity<PackageEntity>().HasIndex(p => p.Name).IsUnique();
        modelBuilder.Entity<PackageVersionEntity>().HasIndex(v => new { v.PackageId, v.Version, v.Platform, v.Arch }).IsUnique();
        modelBuilder.Entity<UserEntity>().HasIndex(u => u.Username).IsUnique();
    }
}

sealed class PackageEntity
{
    public Guid Id { get; set; } = Guid.NewGuid();
    [MaxLength(128)] public required string Name { get; set; }
    [MaxLength(512)] public required string Description { get; set; }
    public List<PackageVersionEntity> Versions { get; set; } = [];
}

sealed class PackageVersionEntity
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public Guid PackageId { get; set; }
    public PackageEntity Package { get; set; } = null!;
    [MaxLength(64)] public required string Version { get; set; }
    [MaxLength(32)] public required string Platform { get; set; }
    [MaxLength(32)] public required string Arch { get; set; }
    [MaxLength(64)] public required string Sha256 { get; set; }
    [MaxLength(256)] public required string Signature { get; set; }
    [MaxLength(1024)] public required string FilePath { get; set; }
    public required string ManifestJson { get; set; }
    public PackageVersionStatus Status { get; set; }
    public long DownloadCount { get; set; }
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
}

enum PackageVersionStatus
{
    Draft,
    Published,
    Unpublished
}

sealed class UserEntity
{
    public Guid Id { get; set; } = Guid.NewGuid();
    [MaxLength(64)] public required string Username { get; set; }
    [MaxLength(512)] public required string PasswordHash { get; set; }
    [MaxLength(32)] public required string Role { get; set; }
    public bool IsActive { get; set; } = true;
}

sealed class AuditLogEntity
{
    public Guid Id { get; set; } = Guid.NewGuid();
    public DateTimeOffset CreatedAt { get; set; } = DateTimeOffset.UtcNow;
    [MaxLength(64)] public required string Actor { get; set; }
    [MaxLength(64)] public required string Action { get; set; }
    [MaxLength(64)] public required string TargetType { get; set; }
    [MaxLength(256)] public required string Target { get; set; }
}

sealed class PackageStorage(IOptions<MtyOptions> options)
{
    private readonly string root = Path.GetFullPath(options.Value.PackageRoot);

    public async Task<StagedPackage> StageAsync(Stream stream)
    {
        Directory.CreateDirectory(root);
        var tempRoot = Path.Combine(root, "_staging");
        Directory.CreateDirectory(tempRoot);
        var fullPath = Path.Combine(tempRoot, $"{Guid.NewGuid():N}.mty");
        await using var output = File.Create(fullPath);
        using var sha = SHA256.Create();
        await using var hashing = new CryptoStream(output, sha, CryptoStreamMode.Write);
        await stream.CopyToAsync(hashing);
        hashing.FlushFinalBlock();
        return new StagedPackage(fullPath, Convert.ToHexString(sha.Hash!).ToLowerInvariant());
    }

    public async Task<StoredPackage> CommitStagedAsync(StagedPackage staged, string name, string version, string platform, string arch)
    {
        var relativePath = Path.Combine(name, version, platform, arch, $"{name}-{version}.mty");
        var fullPath = Resolve(relativePath);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);
        if (File.Exists(fullPath))
        {
            File.Delete(fullPath);
        }
        File.Move(staged.FullPath, fullPath);
        await Task.CompletedTask;
        return new StoredPackage(relativePath, staged.Sha256);
    }

    public async Task<StoredPackage> SaveAsync(string name, string version, string platform, string arch, Stream stream)
    {
        Directory.CreateDirectory(root);
        var relativePath = Path.Combine(name, version, platform, arch, $"{name}-{version}.mty");
        var fullPath = Resolve(relativePath);
        Directory.CreateDirectory(Path.GetDirectoryName(fullPath)!);

        await using var output = File.Create(fullPath);
        using var sha = SHA256.Create();
        await using var hashing = new CryptoStream(output, sha, CryptoStreamMode.Write);
        await stream.CopyToAsync(hashing);
        hashing.FlushFinalBlock();
        return new StoredPackage(relativePath, Convert.ToHexString(sha.Hash!).ToLowerInvariant());
    }

    public async Task<string> ReadManifestJsonAsync(string relativePath)
    {
        return await ReadManifestJsonFromFullPathAsync(Resolve(relativePath));
    }

    public async Task<string> ReadManifestJsonFromFullPathAsync(string fullPath)
    {
        await using var file = File.OpenRead(fullPath);
        using var archive = new System.IO.Compression.ZipArchive(file, System.IO.Compression.ZipArchiveMode.Read);
        var entry = archive.GetEntry("manifest.json") ?? throw new ValidationException("manifest.json is required");
        await using var stream = entry.Open();
        using var reader = new StreamReader(stream);
        return await reader.ReadToEndAsync();
    }

    public string Resolve(string relativePath)
    {
        var fullPath = Path.GetFullPath(Path.Combine(root, relativePath));
        if (!fullPath.StartsWith(root, StringComparison.OrdinalIgnoreCase))
        {
            throw new InvalidOperationException("Invalid package path");
        }
        return fullPath;
    }

    public bool Exists(string relativePath) => File.Exists(Resolve(relativePath));

    public void DeletePackage(string name)
    {
        var fullPath = Resolve(name);
        if (Directory.Exists(fullPath))
        {
            Directory.Delete(fullPath, recursive: true);
        }
    }

    public void DeleteFullPath(string fullPath)
    {
        if (File.Exists(fullPath))
        {
            File.Delete(fullPath);
        }
    }

    public void Delete(string relativePath)
    {
        var fullPath = Resolve(relativePath);
        if (File.Exists(fullPath))
        {
            File.Delete(fullPath);
        }
    }
}

sealed class PackageSigner
{
    private readonly Ed25519PrivateKeyParameters privateKey;
    public string PublicKey { get; }

    public PackageSigner(IOptions<MtyOptions> options)
    {
        var keyPath = Path.GetFullPath(Path.Combine(options.Value.PackageRoot, "signing-key.json"));
        Directory.CreateDirectory(Path.GetDirectoryName(keyPath)!);

        SigningKeyFile keyFile;
        if (File.Exists(keyPath))
        {
            keyFile = JsonSerializer.Deserialize<SigningKeyFile>(File.ReadAllText(keyPath), JsonDefaults.Options)
                ?? throw new InvalidOperationException("Invalid signing-key.json");
        }
        else
        {
            var generated = new Ed25519PrivateKeyParameters(new SecureRandom());
            keyFile = new SigningKeyFile(
                "Ed25519",
                Convert.ToBase64String(generated.GetEncoded()),
                Convert.ToBase64String(generated.GeneratePublicKey().GetEncoded()));
            File.WriteAllText(keyPath, JsonSerializer.Serialize(keyFile, JsonDefaults.Options));
        }

        privateKey = new Ed25519PrivateKeyParameters(Convert.FromBase64String(keyFile.PrivateKey), 0);
        PublicKey = keyFile.PublicKey;
    }

    public string Sign(string sha256)
    {
        var bytes = Encoding.UTF8.GetBytes(sha256);
        var signer = new Ed25519Signer();
        signer.Init(true, privateKey);
        signer.BlockUpdate(bytes, 0, bytes.Length);
        return Convert.ToBase64String(signer.GenerateSignature());
    }
}

sealed class AuthService(IOptions<MtyOptions> options)
{
    public string CreateToken(UserEntity user)
    {
        var jwt = options.Value.Jwt;
        var key = new SymmetricSecurityKey(Encoding.UTF8.GetBytes(jwt.SigningKey));
        var credentials = new SigningCredentials(key, SecurityAlgorithms.HmacSha256);
        var claims = new[]
        {
            new Claim(ClaimTypes.NameIdentifier, user.Id.ToString()),
            new Claim(ClaimTypes.Name, user.Username),
            new Claim(ClaimTypes.Role, user.Role)
        };
        var token = new JwtSecurityToken(jwt.Issuer, jwt.Audience, claims, expires: DateTime.UtcNow.AddHours(8), signingCredentials: credentials);
        return new JwtSecurityTokenHandler().WriteToken(token);
    }
}

static class PasswordHasher
{
    public static string Hash(string password)
    {
        var salt = RandomNumberGenerator.GetBytes(16);
        var hash = Rfc2898DeriveBytes.Pbkdf2(password, salt, 100_000, HashAlgorithmName.SHA256, 32);
        return $"pbkdf2${Convert.ToBase64String(salt)}${Convert.ToBase64String(hash)}";
    }

    public static bool Verify(string password, string stored)
    {
        var parts = stored.Split('$');
        if (parts.Length != 3 || parts[0] != "pbkdf2")
        {
            return false;
        }

        var salt = Convert.FromBase64String(parts[1]);
        var expected = Convert.FromBase64String(parts[2]);
        var actual = Rfc2898DeriveBytes.Pbkdf2(password, salt, 100_000, HashAlgorithmName.SHA256, 32);
        return CryptographicOperations.FixedTimeEquals(actual, expected);
    }
}

sealed record MtyOptions
{
    [Required] public string PackageRoot { get; init; } = "packages";
    [Required] public JwtOptions Jwt { get; init; } = new();
    [Required] public AdminOptions Admin { get; init; } = new();
}

sealed record JwtOptions
{
    [Required] public string Issuer { get; init; } = "mty";
    [Required] public string Audience { get; init; } = "mty-admin";
    [MinLength(32)] public string SigningKey { get; init; } = "replace-with-at-least-32-characters-secret";
}

sealed record AdminOptions
{
    [Required] public string Username { get; init; } = "admin";
    [Required] public string Password { get; init; } = "ChangeMe123!";
}

sealed record LoginRequest(string Username, string Password);
sealed record LoginResponse(string Token, string Username);
sealed record SigningKeyDto(string Algorithm, string PublicKey);
sealed record SigningKeyFile(string Algorithm, string PrivateKey, string PublicKey);
sealed record CreatePackageRequest([Required] string Name, [Required] string Description);
sealed record UpdatePackageRequest([Required] string Name, [Required] string Description);
sealed record UploadVersionRequest(
    [Required] IFormFile File);
sealed class GenerateVersionRequest
{
    [Required] public string Version { get; set; } = string.Empty;
    [Required] public string Platform { get; set; } = string.Empty;
    [Required] public string Arch { get; set; } = string.Empty;
    public string? Description { get; set; }
    [Required] public IFormFile File { get; set; } = null!;
}
sealed record PackageSummaryDto(string Name, string Description, string? LatestVersion);
sealed record PackageDetailDto(string Name, string Description, List<PackageVersionDto> Versions);
sealed record PackageVersionDto(string Version, string Platform, string Arch, string Sha256, string Signature, string DownloadUrl);
sealed record AdminPackageDto(Guid Id, string Name, string Description, int VersionCount, string? LatestVersion);
sealed record AdminPackageDetailDto(string Name, string Description, List<AdminPackageVersionDto> Versions);
sealed record AdminPackageVersionDto(string Version, string Platform, string Arch, string Sha256, string Signature, string Status, long DownloadCount, DateTimeOffset CreatedAt);
sealed record VersionLookupResult(VersionLookupStatus Status, PackageVersionEntity? Entity);
enum VersionLookupStatus
{
    NotFound,
    Found,
    Ambiguous
}
sealed record AuditLogDto(DateTimeOffset CreatedAt, string Actor, string Action, string TargetType, string Target);
sealed record StagedPackage(string FullPath, string Sha256);
sealed record StoredPackage(string RelativePath, string Sha256);
sealed record PackageManifestDto(string Name, string Version, string Platform, string Arch, string? Description);
sealed record GeneratedPackageManifest(
    string Name,
    string Version,
    string Description,
    string Platform,
    string Arch,
    string Entry,
    List<DependencyDto> Dependencies,
    List<GeneratedManifestFile> Files);
sealed record DependencyDto(string Name, string Version);
sealed record GeneratedManifestFile(string Path, string Sha256, bool Executable);

static class JsonDefaults
{
    public static readonly JsonSerializerOptions Options = new(JsonSerializerDefaults.Web);
}

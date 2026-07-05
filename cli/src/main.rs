use anyhow::{anyhow, Context, Result};
use base64::Engine;
use clap::{Parser, Subcommand};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::StatusCode;
use semver::Version;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashMap;
use std::env;
use std::fs::{self, File};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::process::Command;
use tempfile::tempdir;
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Parser)]
#[command(name = "mty", version, about = "MTY package manager")]
struct Cli {
    #[arg(long, env = "MTY_REGISTRY", default_value = "https://mty.itcode.space", global = true)]
    registry: String,
    #[arg(long, env = "MTY_HOME", global = true)]
    home: Option<PathBuf>,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Search { keyword: String },
    Info { name: String },
    Install { name: String, #[arg(long)] version: Option<String> },
    Update { name: Option<String> },
    Remove { name: String },
    List,
    SelfUpdate,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PackageSummary {
    name: String,
    description: String,
    latest_version: Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PackageDetail {
    name: String,
    description: String,
    versions: Vec<PackageVersion>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PackageVersion {
    version: String,
    platform: String,
    arch: String,
    sha256: String,
    signature: String,
    download_url: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
#[serde(rename_all = "camelCase")]
struct PackageManifest {
    name: String,
    version: String,
    description: Option<String>,
    platform: String,
    arch: String,
    entry: String,
    #[serde(default)]
    dependencies: Vec<Dependency>,
    files: Vec<ManifestFile>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct Dependency {
    name: String,
    version: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct ManifestFile {
    path: String,
    sha256: String,
    #[serde(default)]
    executable: bool,
}

#[derive(Debug, Serialize, Deserialize, Default)]
struct LocalState {
    installed: HashMap<String, InstalledPackage>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct InstalledPackage {
    name: String,
    version: String,
    platform: String,
    arch: String,
    entry: String,
    files: Vec<String>,
}

struct Paths {
    root: PathBuf,
    tools: PathBuf,
    bin: PathBuf,
    snapshots: PathBuf,
    state_file: PathBuf,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ProblemDetails {
    title: Option<String>,
    detail: Option<String>,
    status: Option<u16>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SigningKeyDto {
    algorithm: String,
    public_key: String,
}

fn main() -> Result<()> {
    if let Err(error) = run() {
        eprintln!("error: {error}");
        std::process::exit(1);
    }
    Ok(())
}

fn run() -> Result<()> {
    let cli = Cli::parse();
    let paths = resolve_paths(cli.home)?;
    initialize_home(&paths)?;

    let client = Client::new();
    match cli.command {
        Commands::Init => init(&paths),
        Commands::Search { keyword } => search(&client, &cli.registry, &keyword),
        Commands::Info { name } => info(&client, &cli.registry, &name),
        Commands::Install { name, version } => install(&client, &paths, &cli.registry, &name, version.as_deref()),
        Commands::Update { name } => update(&client, &paths, &cli.registry, name.as_deref()),
        Commands::Remove { name } => remove(&paths, &name),
        Commands::List => list(&paths),
        Commands::SelfUpdate => install(&client, &paths, &cli.registry, "mty", None),
    }
}

fn resolve_paths(home: Option<PathBuf>) -> Result<Paths> {
    let root = home
        .or_else(|| env::var_os("MTY_HOME").map(PathBuf::from))
        .or_else(|| dirs::home_dir().map(|p| p.join(".mty")))
        .ok_or_else(|| anyhow!("cannot resolve user home directory"))?;
    Ok(Paths {
        tools: root.join("tools"),
        bin: root.join("bin"),
        snapshots: root.join("snapshots"),
        state_file: root.join("state.json"),
        root,
    })
}

fn initialize_home(paths: &Paths) -> Result<()> {
    fs::create_dir_all(&paths.root).context("unable to create MTY home directory")?;
    fs::create_dir_all(&paths.tools).context("unable to create MTY tools directory")?;
    fs::create_dir_all(&paths.bin).context("unable to create MTY bin directory")?;
    fs::create_dir_all(&paths.snapshots).context("unable to create MTY snapshots directory")?;
    ensure_path_contains(&paths.bin)?;
    Ok(())
}

fn init(paths: &Paths) -> Result<()> {
    initialize_home(paths)?;
    let installed = install_current_mty_executable(paths)?;
    println!("MTY home: {}", paths.root.display());
    println!("Tools: {}", paths.tools.display());
    println!("Command shims: {}", paths.bin.display());
    println!("MTY executable: {}", installed.display());
    println!("Command: mty");
    println!("PATH is configured for future shells. Restart your terminal if commands are not found yet.");
    Ok(())
}

fn search(client: &Client, registry: &str, keyword: &str) -> Result<()> {
    println!("Searching {registry} for \"{keyword}\"...");
    let url = format!("{}/api/packages?keyword={}", registry.trim_end_matches('/'), keyword);
    let packages: Vec<PackageSummary> = send_json(client.get(url), registry)?;
    if packages.is_empty() {
        println!("No packages found.");
        return Ok(());
    }
    println!("{:<28} {:<14} {}", "NAME", "LATEST", "DESCRIPTION");
    for package in packages {
        println!(
            "{:<28} {:<14} {}",
            package.name,
            package.latest_version.unwrap_or_else(|| "-".to_string()),
            package.description
        );
    }
    Ok(())
}

fn info(client: &Client, registry: &str, name: &str) -> Result<()> {
    println!("Fetching package metadata from {registry}...");
    let detail = fetch_detail(client, registry, name)?;
    println!("{}\n{}", detail.name, detail.description);
    if detail.versions.is_empty() {
        println!("No published versions are available.");
        return Ok(());
    }
    println!("{:<14} {:<12} {:<12}", "VERSION", "PLATFORM", "ARCH");
    for version in detail.versions {
        println!("{:<14} {:<12} {:<12}", version.version, version.platform, version.arch);
    }
    Ok(())
}

fn install(client: &Client, paths: &Paths, registry: &str, name: &str, version: Option<&str>) -> Result<()> {
    println!("Resolving {name} from {registry}...");
    let detail = fetch_detail(client, registry, name)?;
    let target = select_version(&detail, version)?;
    println!("Downloading {} {} for {}/{}...", name, target.version, target.platform, target.arch);
    let bytes = client
        .get(expand_url(registry, &target.download_url))
        .send()
        .map_err(|e| friendly_transport_error(registry, e))?;
    let bytes = ensure_success(bytes)?
        .bytes()?;

    println!("Verifying package hash and signature...");
    let actual_hash = sha256_hex(&bytes);
    if actual_hash != target.sha256 {
        return Err(anyhow!("package hash mismatch for {}", name));
    }
    verify_signature(client, registry, &target.sha256, &target.signature)?;

    let tmp = tempdir()?;
    let package_file = tmp.path().join("package.mty");
    fs::write(&package_file, &bytes)?;
    let manifest = extract_package(&package_file, tmp.path())?;
    validate_manifest_files(&manifest, tmp.path())?;
    warn_missing_dependencies(paths, &manifest)?;

    println!("Installing files...");
    let state = load_state(paths)?;
    let snapshot = create_snapshot(paths, &state, &manifest.name)?;
    let result = install_extracted(paths, &manifest, tmp.path());
    if let Err(error) = result {
        restore_snapshot(paths, &manifest.name, snapshot.as_deref())?;
        return Err(error.context("install failed; previous version was restored"));
    }

    let mut state = load_state(paths)?;
    state.installed.insert(
        manifest.name.clone(),
        InstalledPackage {
            name: manifest.name.clone(),
            version: manifest.version.clone(),
            platform: manifest.platform.clone(),
            arch: manifest.arch.clone(),
            entry: manifest.entry.clone(),
            files: manifest.files.iter().map(|f| f.path.clone()).collect(),
        },
    );
    save_state(paths, &state)?;
    create_command_shim(paths, &manifest)?;
    println!("Installed {} {}.", manifest.name, manifest.version);
    println!("Command: {}", manifest.name);
    Ok(())
}

fn update(client: &Client, paths: &Paths, registry: &str, name: Option<&str>) -> Result<()> {
    let state = load_state(paths)?;
    let names: Vec<String> = match name {
        Some(value) => vec![value.to_string()],
        None => state.installed.keys().cloned().collect(),
    };
    for package_name in names {
        println!("Checking {package_name}...");
        let detail = fetch_detail(client, registry, &package_name)?;
        let latest = select_version(&detail, None)?;
        let current = state.installed.get(&package_name).map(|p| p.version.as_str());
        if current == Some(latest.version.as_str()) {
            println!("{package_name} is already current.");
            continue;
        }
        install(client, paths, registry, &package_name, Some(&latest.version))?;
    }
    Ok(())
}

fn remove(paths: &Paths, name: &str) -> Result<()> {
    let mut state = load_state(paths)?;
    let Some(installed) = state.installed.remove(name) else {
        return Err(anyhow!("{} is not installed", name));
    };
    for relative in installed.files {
        let path = paths.tools.join(name).join(relative);
        if path.exists() {
            fs::remove_file(path)?;
        }
    }
    let package_dir = paths.tools.join(name);
    if package_dir.exists() {
        remove_empty_dirs(&package_dir)?;
    }
    remove_command_shim(paths, name)?;
    save_state(paths, &state)?;
    println!("Removed {name}.");
    Ok(())
}

fn list(paths: &Paths) -> Result<()> {
    let state = load_state(paths)?;
    if state.installed.is_empty() {
        println!("No packages installed.");
        return Ok(());
    }
    println!("{:<28} {:<14} {:<12} {}", "NAME", "VERSION", "PLATFORM", "ARCH");
    for package in state.installed.values() {
        println!("{:<28} {:<14} {:<12} {}", package.name, package.version, package.platform, package.arch);
    }
    Ok(())
}

fn fetch_detail(client: &Client, registry: &str, name: &str) -> Result<PackageDetail> {
    let url = format!("{}/api/packages/{}", registry.trim_end_matches('/'), name);
    send_json(client.get(url), registry)
}

fn send_json<T: for<'de> Deserialize<'de>>(request: RequestBuilder, registry: &str) -> Result<T> {
    let response = request
        .send()
        .map_err(|e| friendly_transport_error(registry, e))?;
    Ok(ensure_success(response)?.json()?)
}

fn ensure_success(response: Response) -> Result<Response> {
    let status = response.status();
    if status.is_success() {
        return Ok(response);
    }

    let problem = response.json::<ProblemDetails>().ok();
    let detail = problem
        .as_ref()
        .and_then(|p| p.detail.as_deref())
        .filter(|d| !d.is_empty());
    let title = problem
        .as_ref()
        .and_then(|p| p.title.as_deref())
        .unwrap_or_else(|| status.canonical_reason().unwrap_or("request failed"));
    let problem_status = problem.as_ref().and_then(|p| p.status).unwrap_or(status.as_u16());

    let message = match status {
        StatusCode::UNAUTHORIZED | StatusCode::FORBIDDEN => "authentication failed or you do not have permission".to_string(),
        StatusCode::NOT_FOUND => "not found: package, version, file, or API endpoint does not exist; unpublished versions are hidden from public queries".to_string(),
        StatusCode::CONFLICT => "conflict: this package version probably already exists".to_string(),
        _ => format!("server returned {problem_status}: {title}"),
    };

    if let Some(detail) = detail {
        Err(anyhow!("{message}: {detail}"))
    } else {
        Err(anyhow!("{message}"))
    }
}

fn friendly_transport_error(registry: &str, error: reqwest::Error) -> anyhow::Error {
    if error.is_connect() {
        anyhow!("cannot connect to registry {registry}; start the API with `dotnet run --project server` or set MTY_REGISTRY/--registry")
    } else if error.is_timeout() {
        anyhow!("registry {registry} did not respond in time")
    } else {
        anyhow!(error)
    }
}

fn select_version(detail: &PackageDetail, requested: Option<&str>) -> Result<PackageVersion> {
    if let Some(version) = requested {
        return detail
            .versions
            .iter()
            .find(|v| v.version == version && matches_current_platform(v))
            .cloned()
            .ok_or_else(|| anyhow!("version {} for current platform was not found", version));
    }

    detail
        .versions
        .iter()
        .filter(|v| matches_current_platform(v))
        .max_by_key(|v| Version::parse(&v.version).ok())
        .cloned()
        .ok_or_else(|| anyhow!("no version for current platform was found"))
}

fn matches_current_platform(version: &PackageVersion) -> bool {
    platform_aliases(std::env::consts::OS).contains(&version.platform.as_str())
        && arch_aliases(std::env::consts::ARCH).contains(&version.arch.as_str())
}

fn platform_aliases(platform: &str) -> Vec<&'static str> {
    match platform {
        "windows" => vec!["windows", "win32"],
        "macos" => vec!["macos", "darwin", "osx"],
        "linux" => vec!["linux"],
        _ => vec![],
    }
}

fn arch_aliases(arch: &str) -> Vec<&'static str> {
    match arch {
        "x86_64" => vec!["x86_64", "x64", "amd64"],
        "aarch64" => vec!["aarch64", "arm64"],
        "x86" => vec!["x86", "i386", "i686"],
        _ => vec![],
    }
}

fn expand_url(registry: &str, url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("{}{}", registry.trim_end_matches('/'), url)
    }
}

fn verify_signature(client: &Client, registry: &str, hash: &str, signature: &str) -> Result<()> {
    let signature = signature.trim();
    if signature.is_empty() {
        return Err(anyhow!("package has no signature; upload or re-sign it on the server before installing"));
    }

    let public_key = resolve_public_key(client, registry)?;
    let public_key = base64::engine::general_purpose::STANDARD.decode(public_key)?;
    let signature = base64::engine::general_purpose::STANDARD.decode(signature)?;
    let key_bytes: [u8; 32] = public_key.try_into().map_err(|_| anyhow!("invalid public key length"))?;
    let sig_bytes: [u8; 64] = signature.try_into().map_err(|_| anyhow!("invalid signature length"))?;
    let key = VerifyingKey::from_bytes(&key_bytes)?;
    let sig = Signature::from_bytes(&sig_bytes);
    key.verify(hash.as_bytes(), &sig).context("package signature verification failed")
}

fn resolve_public_key(client: &Client, registry: &str) -> Result<String> {
    if let Ok(public_key) = std::env::var("MTY_PUBLIC_KEY") {
        if !public_key.trim().is_empty() {
            return Ok(public_key);
        }
    }

    let url = format!("{}/api/signing-key", registry.trim_end_matches('/'));
    let key: SigningKeyDto = send_json(client.get(url), registry)?;
    if key.algorithm != "Ed25519" {
        return Err(anyhow!("unsupported registry signing algorithm: {}", key.algorithm));
    }
    Ok(key.public_key)
}

fn extract_package(package_file: &Path, target: &Path) -> Result<PackageManifest> {
    let file = File::open(package_file)?;
    let mut archive = ZipArchive::new(file)?;
    archive.extract(target)?;
    let manifest_text = fs::read_to_string(target.join("manifest.json"))?;
    Ok(serde_json::from_str(&manifest_text)?)
}

fn validate_manifest_files(manifest: &PackageManifest, root: &Path) -> Result<()> {
    for item in &manifest.files {
        let path = root.join(&item.path);
        let bytes = fs::read(&path).with_context(|| format!("missing package file {}", item.path))?;
        if sha256_hex(&bytes) != item.sha256 {
            return Err(anyhow!("file hash mismatch: {}", item.path));
        }
    }
    Ok(())
}

fn warn_missing_dependencies(paths: &Paths, manifest: &PackageManifest) -> Result<()> {
    let state = load_state(paths)?;
    for dependency in &manifest.dependencies {
        if !state.installed.contains_key(&dependency.name) {
            eprintln!("warning: dependency {} {} is not installed", dependency.name, dependency.version);
        }
    }
    Ok(())
}

fn install_extracted(paths: &Paths, manifest: &PackageManifest, source: &Path) -> Result<()> {
    let target_root = paths.tools.join(&manifest.name);
    fs::create_dir_all(&target_root)?;
    for item in &manifest.files {
        let source_path = source.join(&item.path);
        let target_path = target_root.join(&item.path);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source_path, target_path)?;
    }
    Ok(())
}

fn create_command_shim(paths: &Paths, manifest: &PackageManifest) -> Result<()> {
    let entry = paths.tools.join(&manifest.name).join(&manifest.entry);
    if !entry.exists() {
        return Err(anyhow!("entry file does not exist after install: {}", entry.display()));
    }

    write_command_shim(paths, &manifest.name, &entry)
}

fn write_command_shim(paths: &Paths, name: &str, entry: &Path) -> Result<()> {
    fs::create_dir_all(&paths.bin)?;
    let shim = command_shim_path(paths, name);
    let content = command_shim_content(&entry);
    let mut file = File::create(&shim)?;
    file.write_all(content.as_bytes())?;

    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        fs::set_permissions(&shim, fs::Permissions::from_mode(0o755))?;
    }

    Ok(())
}

fn install_current_mty_executable(paths: &Paths) -> Result<PathBuf> {
    let source = env::current_exe().context("cannot resolve current mty executable")?;
    let file_name = source
        .file_name()
        .ok_or_else(|| anyhow!("current executable has no file name"))?;
    let target_dir = paths.tools.join("mty");
    let target = target_dir.join(file_name);

    fs::create_dir_all(&target_dir)
        .with_context(|| format!("unable to create {}", target_dir.display()))?;

    if !same_file_path(&source, &target) {
        fs::copy(&source, &target)
            .with_context(|| format!("unable to install mty executable to {}", target.display()))?;
    }

    write_command_shim(paths, "mty", &target)?;
    Ok(target)
}

fn same_file_path(left: &Path, right: &Path) -> bool {
    match (left.canonicalize(), right.canonicalize()) {
        (Ok(left), Ok(right)) => paths_equal(&left, &right),
        _ => paths_equal(left, right),
    }
}

fn remove_command_shim(paths: &Paths, name: &str) -> Result<()> {
    let shim = command_shim_path(paths, name);
    if shim.exists() {
        fs::remove_file(shim)?;
    }
    Ok(())
}

#[cfg(windows)]
fn command_shim_path(paths: &Paths, name: &str) -> PathBuf {
    paths.bin.join(format!("{name}.cmd"))
}

#[cfg(not(windows))]
fn command_shim_path(paths: &Paths, name: &str) -> PathBuf {
    paths.bin.join(name)
}

#[cfg(windows)]
fn command_shim_content(entry: &Path) -> String {
    format!("@echo off\r\n\"{}\" %*\r\n", entry.display())
}

#[cfg(not(windows))]
fn command_shim_content(entry: &Path) -> String {
    format!("#!/bin/sh\nexec \"{}\" \"$@\"\n", entry.display())
}

fn create_snapshot(paths: &Paths, state: &LocalState, name: &str) -> Result<Option<PathBuf>> {
    if !state.installed.contains_key(name) {
        return Ok(None);
    }
    let source = paths.tools.join(name);
    if !source.exists() {
        return Ok(None);
    }
    let snapshot = paths.snapshots.join(format!("{}-{}", name, chrono_like_timestamp()));
    copy_dir(&source, &snapshot)?;
    Ok(Some(snapshot))
}

fn restore_snapshot(paths: &Paths, name: &str, snapshot: Option<&Path>) -> Result<()> {
    let target = paths.tools.join(name);
    if target.exists() {
        fs::remove_dir_all(&target)?;
    }
    if let Some(snapshot) = snapshot {
        copy_dir(snapshot, &target)?;
    }
    Ok(())
}

fn copy_dir(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in WalkDir::new(source) {
        let entry = entry?;
        let relative = entry.path().strip_prefix(source)?;
        let destination = target.join(relative);
        if entry.file_type().is_dir() {
            fs::create_dir_all(&destination)?;
        } else {
            if let Some(parent) = destination.parent() {
                fs::create_dir_all(parent)?;
            }
            fs::copy(entry.path(), destination)?;
        }
    }
    Ok(())
}

fn remove_empty_dirs(path: &Path) -> Result<()> {
    for entry in WalkDir::new(path).contents_first(true) {
        let entry = entry?;
        if entry.file_type().is_dir() {
            let _ = fs::remove_dir(entry.path());
        }
    }
    Ok(())
}

fn load_state(paths: &Paths) -> Result<LocalState> {
    if !paths.state_file.exists() {
        return Ok(LocalState::default());
    }
    Ok(serde_json::from_str(&fs::read_to_string(&paths.state_file)?)?)
}

fn save_state(paths: &Paths, state: &LocalState) -> Result<()> {
    fs::create_dir_all(&paths.root)?;
    let text = serde_json::to_string_pretty(state)?;
    let mut file = File::create(&paths.state_file)?;
    file.write_all(text.as_bytes())?;
    Ok(())
}

fn ensure_path_contains(bin: &Path) -> Result<()> {
    if path_env_contains(bin) {
        return Ok(());
    }

    persist_path(bin)?;
    Ok(())
}

fn path_env_contains(bin: &Path) -> bool {
    let Ok(path) = env::var("PATH") else {
        return false;
    };
    env::split_paths(&path).any(|p| paths_equal(&p, bin))
}

#[cfg(windows)]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left.to_string_lossy().eq_ignore_ascii_case(&right.to_string_lossy())
}

#[cfg(not(windows))]
fn paths_equal(left: &Path, right: &Path) -> bool {
    left == right
}

#[cfg(windows)]
fn persist_path(bin: &Path) -> Result<()> {
    let bin_text = bin.to_string_lossy().to_string();
    let escaped = bin_text.replace('\'', "''");
    let script = format!(
        "$bin='{escaped}'; $path=[Environment]::GetEnvironmentVariable('Path','User'); \
         if ([string]::IsNullOrWhiteSpace($path)) {{ $new=$bin }} \
         elseif (($path -split ';') -notcontains $bin) {{ $new=$path.TrimEnd(';')+';'+$bin }} \
         else {{ $new=$path }}; \
         [Environment]::SetEnvironmentVariable('Path',$new,'User')"
    );
    let status = Command::new("powershell")
        .args(["-NoProfile", "-ExecutionPolicy", "Bypass", "-Command", &script])
        .status()
        .context("failed to update user PATH with PowerShell")?;
    if !status.success() {
        return Err(anyhow!("failed to update user PATH; add {} manually", bin.display()));
    }
    Ok(())
}

#[cfg(not(windows))]
fn persist_path(bin: &Path) -> Result<()> {
    let profile = dirs::home_dir()
        .ok_or_else(|| anyhow!("cannot resolve user home directory"))?
        .join(".profile");
    let marker_start = "# >>> mty path >>>";
    let marker_end = "# <<< mty path <<<";
    let block = format!(
        "\n{marker_start}\nexport PATH=\"{}:$PATH\"\n{marker_end}\n",
        bin.display()
    );
    let existing = fs::read_to_string(&profile).unwrap_or_default();
    if !existing.contains(marker_start) {
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(&profile)
            .with_context(|| format!("unable to update {}", profile.display()))?;
        file.write_all(block.as_bytes())?;
    }
    Ok(())
}

fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn chrono_like_timestamp() -> String {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs().to_string())
        .unwrap_or_else(|_| "0".to_string())
}

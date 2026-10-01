use anstream::{eprintln, println};
use anyhow::{anyhow, Context, Result};
use base64::Engine;
use clap::builder::styling::{AnsiColor, Style};
use clap::{CommandFactory, FromArgMatches, Parser, Subcommand, ValueEnum};
use ed25519_dalek::{Signature, Verifier, VerifyingKey};
use reqwest::blocking::{Client, RequestBuilder, Response};
use reqwest::StatusCode;
use semver::{Version, VersionReq};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::{HashMap, HashSet};
use std::env;
use std::fmt::{self, Display};
use std::fs::{self, File};
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::process::Command;
use std::time::Duration;
use tempfile::{tempdir, NamedTempFile};
use walkdir::WalkDir;
use zip::ZipArchive;

#[derive(Parser)]
#[command(name = "mty", version, about = "MTY package manager")]
struct Cli {
    #[arg(
        long,
        env = "MTY_REGISTRY",
        default_value = "https://mty.itcode.space",
        global = true
    )]
    registry: String,
    #[arg(long, env = "MTY_HOME", global = true)]
    home: Option<PathBuf>,
    /// Emit a single JSON result; diagnostics go to stderr.
    #[arg(long, global = true)]
    json: bool,
    /// Control terminal colors (JSON and completion scripts stay plain).
    #[arg(long, value_enum, default_value = "auto", global = true)]
    color: ColorMode,
    #[command(subcommand)]
    command: Commands,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, ValueEnum)]
enum ColorMode {
    #[default]
    Auto,
    Always,
    Never,
}

const HEADING: Style = AnsiColor::Blue.on_default().bold();
const NAME: Style = AnsiColor::Cyan.on_default();
const VERSION: Style = AnsiColor::Green.on_default();
const PATH: Style = AnsiColor::Magenta.on_default();
const PROGRESS: Style = AnsiColor::Blue.on_default();
const SUCCESS: Style = AnsiColor::Green.on_default();
const WARNING: Style = AnsiColor::Yellow.on_default();
const ERROR: Style = AnsiColor::Red.on_default();

// Apply field padding to the value, never to the ANSI escape sequences.
struct Styled<T>(Style, T);

impl<T: Display> Display for Styled<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.0)?;
        self.1.fmt(f)?;
        write!(f, "{}", self.0.render_reset())
    }
}

fn parse_cli(args: impl IntoIterator<Item = std::ffi::OsString>) -> Result<Cli, clap::Error> {
    let args: Vec<_> = args.into_iter().collect();
    // Read global output options before Clap renders help or an argument error.
    let options = Cli::command()
        .disable_help_flag(true)
        .disable_help_subcommand(true)
        .disable_version_flag(true)
        .arg(
            clap::Arg::new("help")
                .long("help")
                .short('h')
                .global(true)
                .action(clap::ArgAction::SetTrue),
        )
        .arg(
            clap::Arg::new("display_version")
                .long("version")
                .short('V')
                .action(clap::ArgAction::SetTrue),
        )
        .ignore_errors(true)
        .get_matches_from(&args);
    let color = if options.get_flag("json") {
        ColorMode::Never
    } else {
        *options
            .get_one::<ColorMode>("color")
            .unwrap_or(&ColorMode::Auto)
    };
    let choice = match color {
        ColorMode::Auto => anstream::ColorChoice::Auto,
        ColorMode::Always => anstream::ColorChoice::Always,
        ColorMode::Never => anstream::ColorChoice::Never,
    };
    choice.write_global();
    let clap_color = match color {
        ColorMode::Auto => clap::ColorChoice::Auto,
        ColorMode::Always => clap::ColorChoice::Always,
        ColorMode::Never => clap::ColorChoice::Never,
    };
    let matches = Cli::command()
        .color(clap_color)
        .try_get_matches_from(args)?;
    Cli::from_arg_matches(&matches)
}

#[derive(Subcommand)]
enum Commands {
    Init,
    Search {
        keyword: String,
    },
    Info {
        name: String,
    },
    SystemInfo,
    Install {
        name: String,
        #[arg(long)]
        version: Option<String>,
    },
    Update {
        name: Option<String>,
        #[arg(long)]
        dry_run: bool,
    },
    /// List installed packages with a different available version.
    Outdated {
        name: Option<String>,
    },
    /// Generate shell completion without changing the system.
    Completion {
        #[arg(value_enum)]
        shell: clap_complete::Shell,
    },
    Remove {
        name: String,
    },
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

#[derive(Debug, Serialize, Deserialize, Default, Clone)]
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
    let cli = parse_cli(env::args_os()).unwrap_or_else(|error| error.exit());
    let json = cli.json;
    if let Err(error) = run(cli) {
        if json {
            eprintln!("{}", serde_json::json!({"error": format!("{error:#}")}));
        } else {
            eprintln!("{}", Styled(ERROR, format_args!("error: {error:#}")));
        }
        std::process::exit(1);
    }
    Ok(())
}

fn run(cli: Cli) -> Result<()> {
    if let Commands::SystemInfo = cli.command {
        return system_info(cli.json);
    }
    if let Commands::Completion { shell } = cli.command {
        let mut command = Cli::command();
        if cli.json {
            let mut script = Vec::new();
            clap_complete::generate(shell, &mut command, "mty", &mut script);
            return print_json(
                &serde_json::json!({"shell": shell.to_string(), "script": String::from_utf8(script)?}),
            );
        }
        clap_complete::generate(shell, &mut command, "mty", &mut std::io::stdout());
        return Ok(());
    }

    let paths = resolve_paths(cli.home)?;

    let client = Client::builder()
        .connect_timeout(Duration::from_secs(15))
        .timeout(Duration::from_secs(600))
        .build()?;
    match cli.command {
        Commands::Init => init(&paths, cli.json),
        Commands::Search { keyword } => search(&client, &cli.registry, &keyword, cli.json),
        Commands::Info { name } => info(&client, &cli.registry, &name, cli.json),
        Commands::SystemInfo | Commands::Completion { .. } => unreachable!(),
        Commands::Install { name, version } => {
            let package = install(
                &client,
                &paths,
                &cli.registry,
                &name,
                version.as_deref(),
                cli.json,
            )?;
            if cli.json {
                print_json(&serde_json::json!({"status": "installed", "package": package}))?;
            }
            Ok(())
        }
        Commands::Update { name, dry_run } => update(
            &client,
            &paths,
            &cli.registry,
            name.as_deref(),
            dry_run,
            false,
            cli.json,
        ),
        Commands::Outdated { name } => update(
            &client,
            &paths,
            &cli.registry,
            name.as_deref(),
            true,
            true,
            cli.json,
        ),
        Commands::Remove { name } => remove(&paths, &name, cli.json),
        Commands::List => list(&paths, cli.json),
        Commands::SelfUpdate => {
            let package = install(&client, &paths, &cli.registry, "mty", None, cli.json)?;
            if cli.json {
                print_json(&serde_json::json!({"status": "installed", "package": package}))?;
            }
            Ok(())
        }
    }
}

fn print_json(value: &impl Serialize) -> Result<()> {
    serde_json::to_writer(std::io::stdout().lock(), value)?;
    println!();
    Ok(())
}

macro_rules! status {
    ($json:expr, $style:ident; $($args:tt)*) => {
        if $json { eprintln!($($args)*); }
        else { println!("{}", Styled($style, format_args!($($args)*))); }
    };
    ($json:expr, $($args:tt)*) => {
        status!($json, PROGRESS; $($args)*);
    };
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
    Ok(())
}

fn init(paths: &Paths, json: bool) -> Result<()> {
    initialize_home(paths)?;
    ensure_path_contains(&paths.bin)?;
    let installed = if let Some(package) = load_state(paths)?.installed.get("mty") {
        let entry = paths.tools.join("mty").join(&package.entry);
        if !entry.is_file() {
            return Err(anyhow!(
                "installed mty executable is missing: {}",
                entry.display()
            ));
        }
        write_command_shim(paths, "mty", &entry)?;
        entry
    } else {
        install_current_mty_executable(paths)?
    };
    if json {
        return print_json(
            &serde_json::json!({"status": "initialized", "home": paths.root, "tools": paths.tools, "bin": paths.bin, "executable": installed}),
        );
    }
    println!(
        "{} {}",
        Styled(HEADING, "MTY home:"),
        Styled(PATH, paths.root.display())
    );
    println!(
        "{} {}",
        Styled(HEADING, "Tools:"),
        Styled(PATH, paths.tools.display())
    );
    println!(
        "{} {}",
        Styled(HEADING, "Command shims:"),
        Styled(PATH, paths.bin.display())
    );
    println!(
        "{} {}",
        Styled(HEADING, "MTY executable:"),
        Styled(PATH, installed.display())
    );
    println!("{} {}", Styled(HEADING, "Command:"), Styled(NAME, "mty"));
    println!("{}", Styled(SUCCESS, "PATH is configured for future shells. Restart your terminal if commands are not found yet."));
    Ok(())
}

fn search(client: &Client, registry: &str, keyword: &str, json: bool) -> Result<()> {
    status!(json, "Searching {registry} for \"{keyword}\"...");
    let url = format!("{}/api/packages", registry.trim_end_matches('/'));
    let packages: Vec<PackageSummary> =
        send_json(client.get(url).query(&[("keyword", keyword)]), registry)?;
    if json {
        return print_json(&packages);
    }
    if packages.is_empty() {
        println!("{}", Styled(WARNING, "No packages found."));
        return Ok(());
    }
    println!(
        "{}",
        Styled(
            HEADING,
            format_args!("{:<28} {:<14} {}", "NAME", "LATEST", "DESCRIPTION")
        )
    );
    for package in packages {
        println!(
            "{:<28} {:<14} {}",
            Styled(NAME, package.name),
            Styled(
                VERSION,
                package.latest_version.unwrap_or_else(|| "-".to_string())
            ),
            package.description
        );
    }
    Ok(())
}

fn info(client: &Client, registry: &str, name: &str, json: bool) -> Result<()> {
    status!(json, "Fetching package metadata from {registry}...");
    let detail = fetch_detail(client, registry, name)?;
    if json {
        return print_json(&detail);
    }
    println!(
        "{}\n{}",
        Styled(NAME.bold(), detail.name),
        detail.description
    );
    if detail.versions.is_empty() {
        println!(
            "{}",
            Styled(WARNING, "No published versions are available.")
        );
        return Ok(());
    }
    println!(
        "{}",
        Styled(
            HEADING,
            format_args!("{:<14} {:<12} {:<12}", "VERSION", "PLATFORM", "ARCH")
        )
    );
    for version in detail.versions {
        println!(
            "{:<14} {:<12} {:<12}",
            Styled(VERSION, version.version),
            version.platform,
            version.arch
        );
    }
    Ok(())
}

fn system_info(json: bool) -> Result<()> {
    if json {
        return print_json(&serde_json::json!({
            "host": host_name(), "os": os_display_name(), "platform": env::consts::OS,
            "family": env::consts::FAMILY, "architecture": env::consts::ARCH,
            "memory": memory_info().map(|(total, available)| serde_json::json!({"total": total, "available": available, "used": total.saturating_sub(available)})),
            "storage": storage_info().iter().map(|s| serde_json::json!({"mount": s.mount, "total": s.total, "available": s.available, "used": s.total.saturating_sub(s.available)})).collect::<Vec<_>>()
        }));
    }
    println!("{}", Styled(HEADING, "System information"));
    println!(
        "{:<18} {}",
        Styled(HEADING, "Host"),
        host_name().unwrap_or_else(|| "-".to_string())
    );
    println!(
        "{:<18} {}",
        Styled(HEADING, "OS"),
        os_display_name().unwrap_or_else(|| std::env::consts::OS.to_string())
    );
    println!(
        "{:<18} {}",
        Styled(HEADING, "Platform"),
        std::env::consts::OS
    );
    println!(
        "{:<18} {}",
        Styled(HEADING, "Family"),
        std::env::consts::FAMILY
    );
    println!(
        "{:<18} {}",
        Styled(HEADING, "Architecture"),
        std::env::consts::ARCH
    );

    if let Some((total, available)) = memory_info() {
        println!(
            "{:<18} {}",
            Styled(HEADING, "Memory total"),
            format_bytes(total)
        );
        println!(
            "{:<18} {}",
            Styled(HEADING, "Memory available"),
            format_bytes(available)
        );
        println!(
            "{:<18} {}",
            Styled(HEADING, "Memory used"),
            format_bytes(total.saturating_sub(available))
        );
    } else {
        println!(
            "{:<18} {}",
            Styled(HEADING, "Memory"),
            Styled(WARNING, "unavailable")
        );
    }

    let storage = storage_info();
    if storage.is_empty() {
        println!(
            "{:<18} {}",
            Styled(HEADING, "Storage"),
            Styled(WARNING, "unavailable")
        );
        return Ok(());
    }

    println!();
    println!(
        "{}",
        Styled(
            HEADING,
            format_args!(
                "{:<24} {:>14} {:>14} {:>14}",
                "MOUNT", "TOTAL", "AVAILABLE", "USED"
            )
        )
    );
    for item in storage {
        println!(
            "{:<24} {:>14} {:>14} {:>14}",
            Styled(PATH, item.mount),
            format_bytes(item.total),
            format_bytes(item.available),
            format_bytes(item.total.saturating_sub(item.available))
        );
    }

    Ok(())
}

fn install(
    client: &Client,
    paths: &Paths,
    registry: &str,
    name: &str,
    version: Option<&str>,
    json: bool,
) -> Result<InstalledPackage> {
    validate_package_name(name)?;
    status!(json, "Resolving {name} from {registry}...");
    let detail = fetch_detail(client, registry, name)?;
    let target = select_version(&detail, version)?;
    status!(
        json,
        "Downloading {} {} for {}/{}...",
        name,
        target.version,
        target.platform,
        target.arch
    );
    let response = client
        .get(expand_url(registry, &target.download_url))
        .send()
        .map_err(|e| friendly_transport_error(registry, e))?;
    let mut response = ensure_success(response)?;
    let tmp = tempdir()?;
    let package_file = tmp.path().join("package.mty");
    let mut output = File::create(&package_file)?;
    let mut hash = Sha256::new();
    let mut buffer = [0u8; 64 * 1024];
    loop {
        let count = response
            .read(&mut buffer)
            .context("package download interrupted")?;
        if count == 0 {
            break;
        }
        output.write_all(&buffer[..count])?;
        hash.update(&buffer[..count]);
    }
    drop(output);
    status!(json, "Verifying package hash and signature...");
    if format!("{:x}", hash.finalize()) != target.sha256 {
        return Err(anyhow!("package hash mismatch for {}", name));
    }
    verify_signature(client, registry, &target.sha256, &target.signature)?;

    let extracted = tmp.path().join("extracted");
    let manifest = extract_package(&package_file, &extracted)?;
    validate_manifest_identity(&manifest, name, &target)?;
    validate_manifest_files(&manifest, &extracted)?;

    initialize_home(paths)?;
    ensure_path_contains(&paths.bin)?;
    warn_missing_dependencies(paths, &manifest)?;
    status!(json, "Installing files...");
    let package = commit_install(paths, &manifest, &extracted)?;
    status!(json, SUCCESS; "Installed {} {}.", manifest.name, manifest.version);
    status!(json, NAME; "Command: {}", manifest.name);
    Ok(package)
}

#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct UpdateResult {
    name: String,
    current_version: Option<String>,
    target_version: Option<String>,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<String>,
}

fn update(
    client: &Client,
    paths: &Paths,
    registry: &str,
    name: Option<&str>,
    dry_run: bool,
    outdated: bool,
    json: bool,
) -> Result<()> {
    let state = load_state(paths)?;
    let mut names: Vec<String> = match name {
        Some(value) => vec![value.to_string()],
        None => state.installed.keys().cloned().collect(),
    };
    names.sort();
    let mut results = Vec::new();
    let mut failures = 0;
    for package_name in names {
        status!(json, "Checking {package_name}...");
        let mut result = UpdateResult {
            name: package_name.clone(),
            current_version: state
                .installed
                .get(&package_name)
                .map(|p| p.version.clone()),
            target_version: None,
            status: String::new(),
            error: None,
        };
        let attempt = (|| -> Result<()> {
            if outdated && result.current_version.is_none() {
                return Err(anyhow!("{package_name} is not installed"));
            }
            let detail = fetch_detail(client, registry, &package_name)?;
            let latest = select_version(&detail, None)?;
            result.target_version = Some(latest.version.clone());
            if result.current_version.as_deref() == Some(latest.version.as_str()) {
                result.status = "current".into();
                status!(json, SUCCESS; "{package_name} is already current.");
            } else if dry_run {
                result.status = "available".into();
                status!(
                    json, WARNING;
                    "{}: {} -> {}",
                    package_name,
                    result.current_version.as_deref().unwrap_or("not installed"),
                    latest.version
                );
            } else {
                install(
                    client,
                    paths,
                    registry,
                    &package_name,
                    Some(&latest.version),
                    json,
                )?;
                result.status = "installed".into();
            }
            Ok(())
        })();
        if let Err(error) = attempt {
            failures += 1;
            eprintln!(
                "{}",
                Styled(ERROR, format_args!("error: {package_name}: {error:#}"))
            );
            result.status = "failed".into();
            result.error = Some(format!("{error:#}"));
        }
        if !outdated || result.status != "current" {
            results.push(result);
        }
    }
    if json {
        print_json(&results)?;
    } else if results.is_empty() {
        println!(
            "{}",
            Styled(
                WARNING,
                format_args!(
                    "No packages to {}.",
                    if dry_run { "update" } else { "process" }
                )
            )
        );
    } else {
        println!(
            "{}",
            Styled(
                if failures == 0 { SUCCESS } else { ERROR },
                format_args!("{} package(s) checked; {} failed.", results.len(), failures)
            )
        );
    }
    if failures > 0 {
        return Err(anyhow!(
            "{failures} package(s) failed; other packages were processed"
        ));
    }
    Ok(())
}

fn remove(paths: &Paths, name: &str, json: bool) -> Result<()> {
    validate_package_name(name)?;
    if !paths.state_file.exists() {
        return Err(anyhow!("{} is not installed", name));
    }
    let mut state = load_state(paths)?;
    let Some(installed) = state.installed.remove(name) else {
        return Err(anyhow!("{} is not installed", name));
    };
    let package_dir = paths.tools.join(name);
    ensure_no_symlink(&paths.tools, &safe_relative_path(name)?)?;
    let old_shim = previous_shim(paths, name)?;
    let staged = tempfile::Builder::new()
        .prefix(".remove-")
        .tempdir_in(&paths.tools)?;
    if package_dir.exists() {
        copy_dir(&package_dir, staged.path())?;
        for relative in &installed.files {
            let path = staged.path().join(safe_relative_path(relative)?);
            if path.is_file() {
                fs::remove_file(path)?;
            }
        }
        remove_empty_dirs(staged.path())?;
    }
    let backup = tempfile::Builder::new()
        .prefix("remove-")
        .tempdir_in(&paths.snapshots)?;
    let backup_package = backup.path().join("package");
    let mut moved = false;
    let mut staged_moved = false;
    let result = (|| -> Result<()> {
        if package_dir.exists() {
            fs::rename(&package_dir, &backup_package)
                .context("unable to remove package directory; close running tools and try again")?;
            moved = true;
            if staged.path().exists() {
                fs::rename(staged.path(), &package_dir)?;
                staged_moved = true;
            }
        }
        remove_command_shim(paths, name)?;
        save_state(paths, &state)
    })();
    if let Err(error) = result {
        let rollback = (|| -> Result<()> {
            if staged_moved {
                fs::remove_dir_all(&package_dir)?;
            }
            if moved {
                fs::rename(&backup_package, &package_dir)?;
            }
            restore_shim(paths, name, old_shim.as_deref())
        })();
        if let Err(rollback) = rollback {
            let retained = backup.keep();
            return Err(anyhow!(
                "{error:#}; rollback failed: {rollback:#}; backup retained at {}",
                retained.display()
            ));
        }
        return Err(error.context("remove failed; previous installation was restored"));
    }
    if json {
        print_json(&serde_json::json!({"status": "removed", "name": name}))?;
    } else {
        println!("{}", Styled(SUCCESS, format_args!("Removed {name}.")));
    }
    Ok(())
}

fn list(paths: &Paths, json: bool) -> Result<()> {
    let state = load_state(paths)?;
    let mut packages: Vec<_> = state.installed.values().collect();
    packages.sort_by(|a, b| a.name.cmp(&b.name));
    if json {
        return print_json(&packages);
    }
    if state.installed.is_empty() {
        println!("{}", Styled(WARNING, "No packages installed."));
        return Ok(());
    }
    println!(
        "{}",
        Styled(
            HEADING,
            format_args!(
                "{:<28} {:<14} {:<12} {}",
                "NAME", "VERSION", "PLATFORM", "ARCH"
            )
        )
    );
    for package in packages {
        println!(
            "{:<28} {:<14} {:<12} {}",
            Styled(NAME, &package.name),
            Styled(VERSION, &package.version),
            package.platform,
            package.arch
        );
    }
    Ok(())
}

struct StorageInfo {
    mount: String,
    total: u64,
    available: u64,
}

fn host_name() -> Option<String> {
    env::var("COMPUTERNAME")
        .or_else(|_| env::var("HOSTNAME"))
        .ok()
        .filter(|value| !value.trim().is_empty())
        .or_else(|| run_command("hostname", &[]).map(|value| value.trim().to_string()))
}

#[cfg(windows)]
fn os_display_name() -> Option<String> {
    let key = r"HKLM\SOFTWARE\Microsoft\Windows NT\CurrentVersion";
    let name = registry_value(key, "ProductName").unwrap_or_else(|| "Windows".to_string());
    let version = registry_value(key, "DisplayVersion")
        .or_else(|| registry_value(key, "ReleaseId"))
        .unwrap_or_else(|| "-".to_string());
    let build = registry_value(key, "CurrentBuildNumber").unwrap_or_else(|| "-".to_string());
    Some(format!("{name} {version} (build {build})"))
}

#[cfg(target_os = "macos")]
fn os_display_name() -> Option<String> {
    let name = run_command("sw_vers", &["-productName"])?;
    let version = run_command("sw_vers", &["-productVersion"])?;
    let build = run_command("sw_vers", &["-buildVersion"])?;
    Some(format!(
        "{} {} (build {})",
        name.trim(),
        version.trim(),
        build.trim()
    ))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn os_display_name() -> Option<String> {
    let os_release = fs::read_to_string("/etc/os-release").ok()?;
    for line in os_release.lines() {
        if let Some(value) = line.strip_prefix("PRETTY_NAME=") {
            return Some(value.trim_matches('"').to_string());
        }
    }
    None
}

#[cfg(windows)]
fn memory_info() -> Option<(u64, u64)> {
    let mut status = MemoryStatusEx::default();
    status.length = std::mem::size_of::<MemoryStatusEx>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return None;
    }
    Some((status.total_phys, status.avail_phys))
}

#[cfg(target_os = "macos")]
fn memory_info() -> Option<(u64, u64)> {
    let total = run_command("sysctl", &["-n", "hw.memsize"])?
        .trim()
        .parse::<u64>()
        .ok()?;
    let vm_stat = run_command("vm_stat", &[])?;
    let mut page_size = 4096u64;
    let mut free_pages = 0u64;
    for line in vm_stat.lines() {
        if let Some(size) = line.strip_prefix("Mach Virtual Memory Statistics: (page size of ") {
            page_size = size.trim_end_matches(" bytes)").parse().ok()?;
        } else if let Some(value) = line.strip_prefix("Pages free:") {
            free_pages = parse_vm_stat_pages(value)?;
        } else if let Some(value) = line.strip_prefix("Pages inactive:") {
            free_pages = free_pages.saturating_add(parse_vm_stat_pages(value)?);
        }
    }
    Some((total, free_pages.saturating_mul(page_size)))
}

#[cfg(all(unix, not(target_os = "macos")))]
fn memory_info() -> Option<(u64, u64)> {
    let text = fs::read_to_string("/proc/meminfo").ok()?;
    let mut total = None;
    let mut available = None;
    for line in text.lines() {
        if line.starts_with("MemTotal:") {
            total = parse_meminfo_kb(line);
        } else if line.starts_with("MemAvailable:") {
            available = parse_meminfo_kb(line);
        }
    }
    Some((total?, available?))
}

#[cfg(windows)]
fn storage_info() -> Vec<StorageInfo> {
    logical_drives()
        .into_iter()
        .filter_map(|drive| {
            let wide = wide_null(&drive);
            let drive_type = unsafe { GetDriveTypeW(wide.as_ptr()) };
            if drive_type != DRIVE_FIXED {
                return None;
            }

            let mut available = 0u64;
            let mut total = 0u64;
            let mut free = 0u64;
            let ok = unsafe {
                GetDiskFreeSpaceExW(wide.as_ptr(), &mut available, &mut total, &mut free)
            };
            if ok == 0 {
                return None;
            }

            Some(StorageInfo {
                mount: drive.trim_end_matches('\\').to_string(),
                total,
                available,
            })
        })
        .collect()
}

#[cfg(windows)]
fn logical_drives() -> Vec<String> {
    let len = unsafe { GetLogicalDriveStringsW(0, std::ptr::null_mut()) };
    if len == 0 {
        return Vec::new();
    }

    let mut buffer = vec![0u16; len as usize + 1];
    let written = unsafe { GetLogicalDriveStringsW(buffer.len() as u32, buffer.as_mut_ptr()) };
    if written == 0 {
        return Vec::new();
    }

    let mut drives = Vec::new();
    let mut start = 0usize;
    for index in 0..buffer.len() {
        if buffer[index] == 0 {
            if index == start {
                break;
            }
            drives.push(String::from_utf16_lossy(&buffer[start..index]));
            start = index + 1;
        }
    }
    drives
}

#[cfg(unix)]
fn storage_info() -> Vec<StorageInfo> {
    let Some(output) = run_command("df", &["-kP"]) else {
        return Vec::new();
    };
    output
        .lines()
        .skip(1)
        .filter_map(|line| {
            let parts: Vec<&str> = line.split_whitespace().collect();
            if parts.len() < 6 {
                return None;
            }
            let total = parts[1].parse::<u64>().ok()?.saturating_mul(1024);
            let available = parts[3].parse::<u64>().ok()?.saturating_mul(1024);
            Some(StorageInfo {
                mount: parts[5].to_string(),
                total,
                available,
            })
        })
        .collect()
}

fn run_command(program: &str, args: &[&str]) -> Option<String> {
    let output = Command::new(program).args(args).output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(String::from_utf8_lossy(&output.stdout).trim().to_string())
}

#[cfg(windows)]
fn registry_value(key: &str, name: &str) -> Option<String> {
    let output = run_command("reg", &["query", key, "/v", name])?;
    output.lines().find_map(|line| {
        let trimmed = line.trim();
        if !trimmed.starts_with(name) {
            return None;
        }
        let parts: Vec<&str> = trimmed.split_whitespace().collect();
        if parts.len() < 3 {
            return None;
        }
        Some(parts[2..].join(" "))
    })
}

#[cfg(windows)]
fn wide_null(value: &str) -> Vec<u16> {
    use std::os::windows::ffi::OsStrExt;
    std::ffi::OsStr::new(value)
        .encode_wide()
        .chain(Some(0))
        .collect()
}

#[cfg(windows)]
#[repr(C)]
#[derive(Default)]
struct MemoryStatusEx {
    length: u32,
    memory_load: u32,
    total_phys: u64,
    avail_phys: u64,
    total_page_file: u64,
    avail_page_file: u64,
    total_virtual: u64,
    avail_virtual: u64,
    avail_extended_virtual: u64,
}

#[cfg(windows)]
const DRIVE_FIXED: u32 = 3;

#[cfg(windows)]
#[link(name = "kernel32")]
extern "system" {
    fn GlobalMemoryStatusEx(buffer: *mut MemoryStatusEx) -> i32;
    fn GetLogicalDriveStringsW(buffer_length: u32, buffer: *mut u16) -> u32;
    fn GetDriveTypeW(root_path_name: *const u16) -> u32;
    fn GetDiskFreeSpaceExW(
        directory_name: *const u16,
        free_bytes_available_to_caller: *mut u64,
        total_number_of_bytes: *mut u64,
        total_number_of_free_bytes: *mut u64,
    ) -> i32;
}

#[cfg(all(unix, not(target_os = "macos")))]
fn parse_meminfo_kb(line: &str) -> Option<u64> {
    line.split_whitespace()
        .nth(1)?
        .parse::<u64>()
        .ok()
        .map(|kb| kb.saturating_mul(1024))
}

#[cfg(target_os = "macos")]
fn parse_vm_stat_pages(value: &str) -> Option<u64> {
    value.trim().trim_end_matches('.').parse::<u64>().ok()
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KiB", "MiB", "GiB", "TiB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{} {}", bytes, UNITS[unit])
    } else {
        format!("{value:.2} {}", UNITS[unit])
    }
}

fn fetch_detail(client: &Client, registry: &str, name: &str) -> Result<PackageDetail> {
    validate_package_name(name)?;
    let mut url =
        reqwest::Url::parse(&format!("{}/api/packages/", registry.trim_end_matches('/')))?;
    url.path_segments_mut()
        .map_err(|_| anyhow!("invalid registry URL"))?
        .pop_if_empty()
        .push(name);
    send_json(client.get(url), registry)
}

fn send_json<T: for<'de> Deserialize<'de>>(request: RequestBuilder, registry: &str) -> Result<T> {
    let response = request
        .timeout(Duration::from_secs(30))
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
    let problem_status = problem
        .as_ref()
        .and_then(|p| p.status)
        .unwrap_or(status.as_u16());

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
        return Err(anyhow!(
            "package has no signature; upload or re-sign it on the server before installing"
        ));
    }

    let public_key = resolve_public_key(client, registry)?;
    let public_key = base64::engine::general_purpose::STANDARD.decode(public_key)?;
    let signature = base64::engine::general_purpose::STANDARD.decode(signature)?;
    let key_bytes: [u8; 32] = public_key
        .try_into()
        .map_err(|_| anyhow!("invalid public key length"))?;
    let sig_bytes: [u8; 64] = signature
        .try_into()
        .map_err(|_| anyhow!("invalid signature length"))?;
    let key = VerifyingKey::from_bytes(&key_bytes)?;
    let sig = Signature::from_bytes(&sig_bytes);
    key.verify(hash.as_bytes(), &sig)
        .context("package signature verification failed")
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
        return Err(anyhow!(
            "unsupported registry signing algorithm: {}",
            key.algorithm
        ));
    }
    Ok(key.public_key)
}

fn extract_package(package_file: &Path, target: &Path) -> Result<PackageManifest> {
    let file = File::open(package_file)?;
    let mut archive = ZipArchive::new(file)?;
    let mut names = HashSet::new();
    // Validate every archive member before writing any extracted files.
    for index in 0..archive.len() {
        let item = archive.by_index(index)?;
        let relative = safe_relative_path(item.name().trim_end_matches('/'))?;
        if !names.insert(path_key(&relative)) {
            return Err(anyhow!("duplicate archive path: {}", item.name()));
        }
        if item
            .unix_mode()
            .is_some_and(|mode| mode & 0o170000 == 0o120000)
        {
            return Err(anyhow!(
                "symbolic links are not allowed in packages: {}",
                item.name()
            ));
        }
    }
    for index in 0..archive.len() {
        let mut item = archive.by_index(index)?;
        let destination = target.join(safe_relative_path(item.name().trim_end_matches('/'))?);
        if item.is_dir() {
            fs::create_dir_all(&destination)?;
        } else {
            fs::create_dir_all(destination.parent().context("archive file has no parent")?)?;
            std::io::copy(&mut item, &mut File::create(&destination)?)?;
        }
    }
    let manifest_text = fs::read_to_string(target.join("manifest.json"))?;
    Ok(serde_json::from_str(&manifest_text)?)
}

fn validate_package_name(name: &str) -> Result<()> {
    let path = safe_relative_path(name)?;
    if path.components().count() != 1 || name.contains('/') || name.contains('\\') {
        return Err(anyhow!("invalid package name: {name}"));
    }
    Ok(())
}

fn safe_relative_path(value: &str) -> Result<PathBuf> {
    let normalized = value.replace('\\', "/");
    if normalized.is_empty()
        || normalized.starts_with('/')
        || normalized.contains(':')
        || normalized.chars().any(char::is_control)
    {
        return Err(anyhow!("unsafe package path: {value:?}"));
    }
    let mut path = PathBuf::new();
    for part in normalized.split('/') {
        if part == ".." {
            return Err(anyhow!("unsafe package path: {value:?}"));
        }
        if part.is_empty() || part == "." {
            continue;
        }
        #[cfg(windows)]
        {
            let stem = part.split('.').next().unwrap_or("").to_ascii_uppercase();
            if part.ends_with('.')
                || part.ends_with(' ')
                || part
                    .chars()
                    .any(|c| matches!(c, '<' | '>' | '"' | '|' | '?' | '*'))
                || matches!(stem.as_str(), "CON" | "PRN" | "AUX" | "NUL")
                || (stem.len() == 4
                    && (stem.starts_with("COM") || stem.starts_with("LPT"))
                    && stem.as_bytes()[3].is_ascii_digit())
            {
                return Err(anyhow!("unsafe Windows package path: {value:?}"));
            }
        }
        path.push(part);
    }
    if path.as_os_str().is_empty() {
        return Err(anyhow!("empty package path: {value:?}"));
    }
    Ok(path)
}

fn path_key(path: &Path) -> String {
    let value = path.to_string_lossy().to_string();
    if cfg!(windows) {
        value.to_lowercase()
    } else {
        value
    }
}

fn ensure_no_symlink(root: &Path, relative: &Path) -> Result<()> {
    let mut current = root.to_path_buf();
    // MTY_HOME may itself be a user-selected symlink; reject links inside packages.
    for part in relative.components() {
        current.push(part);
        match fs::symlink_metadata(&current) {
            Ok(metadata) if metadata.file_type().is_symlink() => {
                return Err(anyhow!(
                    "symbolic link in package path: {}",
                    current.display()
                ))
            }
            Ok(_) => {}
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
            Err(error) => return Err(error.into()),
        }
    }
    Ok(())
}

fn validate_manifest_identity(
    manifest: &PackageManifest,
    name: &str,
    target: &PackageVersion,
) -> Result<()> {
    validate_package_name(&manifest.name)?;
    if !manifest.name.eq_ignore_ascii_case(name)
        || manifest.version != target.version
        || manifest.platform != target.platform
        || manifest.arch != target.arch
        || !matches_current_platform(target)
    {
        return Err(anyhow!(
            "manifest identity does not match the requested package/version/platform"
        ));
    }
    Ok(())
}

fn validate_manifest_files(manifest: &PackageManifest, root: &Path) -> Result<()> {
    validate_package_name(&manifest.name)?;
    let entry = path_key(&safe_relative_path(&manifest.entry)?);
    let mut files = HashSet::new();
    for item in &manifest.files {
        let relative = safe_relative_path(&item.path)?;
        if !files.insert(path_key(&relative)) {
            return Err(anyhow!("duplicate manifest path: {}", item.path));
        }
        ensure_no_symlink(root, &relative)?;
        let path = root.join(relative);
        let mut file =
            File::open(&path).with_context(|| format!("missing package file {}", item.path))?;
        let mut hasher = Sha256::new();
        let mut buffer = [0u8; 64 * 1024];
        loop {
            let count = file.read(&mut buffer)?;
            if count == 0 {
                break;
            }
            hasher.update(&buffer[..count]);
        }
        if format!("{:x}", hasher.finalize()) != item.sha256 {
            return Err(anyhow!("file hash mismatch: {}", item.path));
        }
    }
    if !files.contains(&entry) {
        return Err(anyhow!(
            "entry must be a verified file listed in manifest.files"
        ));
    }
    Ok(())
}

fn warn_missing_dependencies(paths: &Paths, manifest: &PackageManifest) -> Result<()> {
    let state = load_state(paths)?;
    for dependency in &manifest.dependencies {
        match state.installed.get(&dependency.name) {
            None => eprintln!(
                "{}",
                Styled(
                    WARNING,
                    format_args!(
                        "warning: dependency {} {} is not installed",
                        dependency.name, dependency.version
                    )
                )
            ),
            Some(installed) => {
                if let Some(warning) = dependency_warning(dependency, &installed.version) {
                    eprintln!("{}", Styled(WARNING, format_args!("warning: {warning}")));
                }
            }
        }
    }
    Ok(())
}

fn dependency_warning(dependency: &Dependency, installed: &str) -> Option<String> {
    match (
        VersionReq::parse(&dependency.version),
        Version::parse(installed),
    ) {
        (Ok(requirement), Ok(version)) if requirement.matches(&version) => None,
        (Ok(_), Ok(_)) => Some(format!(
            "dependency {} requires {}; installed {}",
            dependency.name, dependency.version, installed
        )),
        _ => Some(format!(
            "cannot verify dependency {} requirement {} against installed {}",
            dependency.name, dependency.version, installed
        )),
    }
}

fn install_extracted(target_root: &Path, manifest: &PackageManifest, source: &Path) -> Result<()> {
    fs::create_dir_all(target_root)?;
    for item in &manifest.files {
        let relative = safe_relative_path(&item.path)?;
        ensure_no_symlink(target_root, &relative)?;
        let source_path = source.join(&relative);
        let target_path = target_root.join(&relative);
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent)?;
        }
        fs::copy(source_path, &target_path)?;
        #[cfg(unix)]
        if item.executable || relative == safe_relative_path(&manifest.entry)? {
            use std::os::unix::fs::PermissionsExt;
            fs::set_permissions(&target_path, fs::Permissions::from_mode(0o755))?;
        }
    }
    Ok(())
}

fn write_command_shim(paths: &Paths, name: &str, entry: &Path) -> Result<()> {
    fs::create_dir_all(&paths.bin)?;
    let shim = command_shim_path(paths, name);
    let content = command_shim_content(&entry);
    atomic_write(&shim, content.as_bytes(), true)
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
    format!(
        "@echo off\r\n\"{}\" %*\r\n",
        entry.display().to_string().replace('%', "%%")
    )
}

#[cfg(not(windows))]
fn command_shim_content(entry: &Path) -> String {
    format!(
        "#!/bin/sh\nexec '{}' \"$@\"\n",
        entry.display().to_string().replace('\'', "'\"'\"'")
    )
}

fn atomic_write(path: &Path, bytes: &[u8], executable: bool) -> Result<()> {
    let parent = path.parent().context("file has no parent directory")?;
    let mut file = NamedTempFile::new_in(parent)?;
    file.write_all(bytes)?;
    #[cfg(unix)]
    if executable {
        use std::os::unix::fs::PermissionsExt;
        file.as_file()
            .set_permissions(fs::Permissions::from_mode(0o755))?;
    }
    #[cfg(not(unix))]
    let _ = executable;
    file.as_file().sync_all()?;
    file.persist(path)
        .map_err(|error| error.error)
        .with_context(|| format!("unable to replace {}", path.display()))?;
    Ok(())
}

fn previous_shim(paths: &Paths, name: &str) -> Result<Option<Vec<u8>>> {
    match fs::read(command_shim_path(paths, name)) {
        Ok(bytes) => Ok(Some(bytes)),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(error.into()),
    }
}

fn restore_shim(paths: &Paths, name: &str, previous: Option<&[u8]>) -> Result<()> {
    match previous {
        Some(bytes) => atomic_write(&command_shim_path(paths, name), bytes, true),
        None => remove_command_shim(paths, name),
    }
}

fn commit_install(
    paths: &Paths,
    manifest: &PackageManifest,
    source: &Path,
) -> Result<InstalledPackage> {
    let mut state = load_state(paths)?;
    #[cfg(windows)]
    if state
        .installed
        .keys()
        .any(|name| name != &manifest.name && name.eq_ignore_ascii_case(&manifest.name))
    {
        return Err(anyhow!(
            "package name differs only in case from an installed package"
        ));
    }
    let name = &manifest.name;
    let target = paths.tools.join(name);
    ensure_no_symlink(&paths.tools, &safe_relative_path(name)?)?;
    let old_shim = previous_shim(paths, name)?;
    let mut installed = InstalledPackage {
        name: name.clone(),
        version: manifest.version.clone(),
        platform: manifest.platform.clone(),
        arch: manifest.arch.clone(),
        entry: safe_relative_path(&manifest.entry)?
            .to_string_lossy()
            .into_owned(),
        files: manifest
            .files
            .iter()
            .map(|file| {
                safe_relative_path(&file.path).map(|path| path.to_string_lossy().into_owned())
            })
            .collect::<Result<_>>()?,
    };

    // A running mty executable must never be overwritten (especially on Windows).
    if name.eq_ignore_ascii_case("mty") {
        fs::create_dir_all(&target)?;
        let release = tempfile::Builder::new()
            .prefix("release-")
            .tempdir_in(&target)?;
        install_extracted(release.path(), manifest, source)?;
        let prefix = release
            .path()
            .file_name()
            .context("release directory has no name")?;
        installed.entry = Path::new(prefix)
            .join(&installed.entry)
            .to_string_lossy()
            .into_owned();
        installed.files = installed
            .files
            .iter()
            .map(|file| Path::new(prefix).join(file).to_string_lossy().into_owned())
            .collect();
        state.installed.insert(name.clone(), installed.clone());
        let result = write_command_shim(paths, name, &target.join(&installed.entry))
            .and_then(|_| save_state(paths, &state));
        if let Err(error) = result {
            if let Err(rollback) = restore_shim(paths, name, old_shim.as_deref()) {
                let retained = release.keep();
                return Err(anyhow!(
                    "{error:#}; restoring command failed: {rollback:#}; release retained at {}",
                    retained.display()
                ));
            }
            return Err(error.context("self-update failed; previous command was restored"));
        }
        // ponytail: Keep older releases; safe cleanup needs process-aware handoff on Windows.
        let _ = release.keep();
        return Ok(installed);
    }

    // ponytail: Copy the package tree so user files survive upgrades; change if its disk cost becomes material.
    let staged = tempfile::Builder::new()
        .prefix(".install-")
        .tempdir_in(&paths.tools)?;
    if target.exists() {
        copy_dir(&target, staged.path())?;
        if let Some(previous) = state.installed.get(name) {
            for file in &previous.files {
                let path = staged.path().join(safe_relative_path(file)?);
                if path.is_file() {
                    fs::remove_file(path)?;
                }
            }
            remove_empty_dirs(staged.path())?;
        }
    }
    install_extracted(staged.path(), manifest, source)?;
    let backup = tempfile::Builder::new()
        .prefix(&format!("{name}-"))
        .tempdir_in(&paths.snapshots)?;
    let backup_package = backup.path().join("package");
    let mut old_moved = false;
    let mut new_moved = false;
    let result = (|| -> Result<()> {
        if target.exists() {
            fs::rename(&target, &backup_package)
                .context("unable to move previous package; close running tools and try again")?;
            old_moved = true;
        }
        fs::rename(staged.path(), &target)?;
        new_moved = true;
        write_command_shim(paths, name, &target.join(&installed.entry))?;
        state.installed.insert(name.clone(), installed.clone());
        save_state(paths, &state)
    })();
    if let Err(error) = result {
        let rollback = (|| -> Result<()> {
            if new_moved {
                fs::remove_dir_all(&target)?;
            }
            if old_moved {
                fs::rename(&backup_package, &target)?;
            }
            restore_shim(paths, name, old_shim.as_deref())
        })();
        if let Err(rollback) = rollback {
            let retained = backup.keep();
            return Err(anyhow!(
                "{error:#}; rollback failed: {rollback:#}; backup retained at {}",
                retained.display()
            ));
        }
        return Err(error.context("install failed; previous version was restored"));
    }
    if old_moved {
        let _ = backup.keep();
    }
    Ok(installed)
}

fn validate_state_paths(state: &LocalState) -> Result<()> {
    for (name, package) in &state.installed {
        validate_package_name(name)?;
        if name != &package.name {
            return Err(anyhow!("installed package name does not match state key"));
        }
        safe_relative_path(&package.entry)?;
        for file in &package.files {
            safe_relative_path(file)?;
        }
    }
    Ok(())
}

fn copy_dir(source: &Path, target: &Path) -> Result<()> {
    fs::create_dir_all(target)?;
    for entry in WalkDir::new(source) {
        let entry = entry?;
        if entry.file_type().is_symlink() {
            return Err(anyhow!(
                "symbolic link in existing package: {}",
                entry.path().display()
            ));
        }
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
    let state = serde_json::from_str(&fs::read_to_string(&paths.state_file)?)?;
    validate_state_paths(&state)?;
    Ok(state)
}

fn save_state(paths: &Paths, state: &LocalState) -> Result<()> {
    fs::create_dir_all(&paths.root)?;
    let text = serde_json::to_string_pretty(state)?;
    atomic_write(&paths.state_file, text.as_bytes(), false)
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
    left.to_string_lossy()
        .eq_ignore_ascii_case(&right.to_string_lossy())
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
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &script,
        ])
        .status()
        .context("failed to update user PATH with PowerShell")?;
    if !status.success() {
        return Err(anyhow!(
            "failed to update user PATH; add {} manually",
            bin.display()
        ));
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

#[cfg(test)]
fn sha256_hex(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn styles_preserve_padding_and_reset() {
        assert_eq!(
            format!("{:<8}|{:>8}", Styled(NAME, "demo"), Styled(VERSION, "1.0")),
            "\x1b[36mdemo    \x1b[0m|\x1b[32m     1.0\x1b[0m"
        );
    }

    fn paths(root: &Path) -> Paths {
        Paths {
            root: root.to_path_buf(),
            tools: root.join("tools"),
            bin: root.join("bin"),
            snapshots: root.join("snapshots"),
            state_file: root.join("state.json"),
        }
    }

    fn manifest(bytes: &[u8]) -> PackageManifest {
        PackageManifest {
            name: "demo".into(),
            version: "2.0.0".into(),
            description: None,
            platform: env::consts::OS.into(),
            arch: env::consts::ARCH.into(),
            entry: "bin/demo".into(),
            dependencies: vec![],
            files: vec![ManifestFile {
                path: "bin/demo".into(),
                sha256: sha256_hex(bytes),
                executable: true,
            }],
        }
    }

    #[test]
    fn rejects_paths_that_escape_package_root() {
        for value in [
            "../outside",
            "a/../../outside",
            "/rooted",
            "C:/outside",
            "a\\..\\outside",
        ] {
            assert!(safe_relative_path(value).is_err(), "accepted {value}");
        }
        assert!(validate_package_name("folder/tool").is_err());
        assert!(validate_package_name("..").is_err());
        assert_eq!(
            safe_relative_path("bin\\tool").unwrap(),
            PathBuf::from("bin/tool")
        );
    }

    #[test]
    fn verifies_declared_files_and_entry() {
        let temp = tempdir().unwrap();
        fs::create_dir_all(temp.path().join("bin")).unwrap();
        fs::write(temp.path().join("bin/demo"), b"good").unwrap();
        let manifest = manifest(b"good");
        validate_manifest_files(&manifest, temp.path()).unwrap();

        let mut bad_hash = manifest.clone();
        bad_hash.files[0].sha256 = sha256_hex(b"bad");
        assert!(validate_manifest_files(&bad_hash, temp.path()).is_err());
        let mut missing_entry = manifest;
        missing_entry.entry = "bin/other".into();
        assert!(validate_manifest_files(&missing_entry, temp.path()).is_err());
    }

    #[test]
    fn dependency_mismatch_warns_without_blocking() {
        let dependency = Dependency {
            name: "runtime".into(),
            version: ">=2.0.0".into(),
        };
        assert_eq!(dependency_warning(&dependency, "2.1.0"), None);
        assert!(dependency_warning(&dependency, "1.9.0").is_some());
    }

    #[test]
    fn failed_state_write_restores_package_and_shim() {
        let temp = tempdir().unwrap();
        let mut paths = paths(temp.path());
        initialize_home(&paths).unwrap();
        let package = paths.tools.join("demo");
        fs::create_dir_all(&package).unwrap();
        fs::write(package.join("old.txt"), b"old").unwrap();
        let shim = command_shim_path(&paths, "demo");
        fs::write(&shim, b"old shim").unwrap();
        paths.state_file = paths.root.join("missing/state.json");
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/demo"), b"new").unwrap();

        assert!(commit_install(&paths, &manifest(b"new"), &source).is_err());
        assert_eq!(fs::read(package.join("old.txt")).unwrap(), b"old");
        assert!(!package.join("bin/demo").exists());
        assert_eq!(fs::read(shim).unwrap(), b"old shim");
    }

    #[test]
    fn install_preserves_unmanaged_files_and_replaces_managed_files() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        initialize_home(&paths).unwrap();
        let package = paths.tools.join("demo");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(package.join("bin/demo"), b"old").unwrap();
        fs::write(package.join("settings.ini"), b"user setting").unwrap();
        let mut state = LocalState::default();
        state.installed.insert(
            "demo".into(),
            InstalledPackage {
                name: "demo".into(),
                version: "1.0.0".into(),
                platform: env::consts::OS.into(),
                arch: env::consts::ARCH.into(),
                entry: "bin/demo".into(),
                files: vec!["bin/demo".into()],
            },
        );
        save_state(&paths, &state).unwrap();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/demo"), b"new").unwrap();

        commit_install(&paths, &manifest(b"new"), &source).unwrap();
        assert_eq!(fs::read(package.join("bin/demo")).unwrap(), b"new");
        assert_eq!(
            fs::read(package.join("settings.ini")).unwrap(),
            b"user setting"
        );
        assert_eq!(
            load_state(&paths).unwrap().installed["demo"].version,
            "2.0.0"
        );
    }

    #[test]
    fn remove_deletes_managed_files_and_keeps_user_settings() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        initialize_home(&paths).unwrap();
        let package = paths.tools.join("demo");
        fs::create_dir_all(package.join("bin")).unwrap();
        fs::write(package.join("bin/demo"), b"managed").unwrap();
        fs::write(package.join("settings.ini"), b"user setting").unwrap();
        let mut state = LocalState::default();
        state.installed.insert(
            "demo".into(),
            InstalledPackage {
                name: "demo".into(),
                version: "1.0.0".into(),
                platform: env::consts::OS.into(),
                arch: env::consts::ARCH.into(),
                entry: "bin/demo".into(),
                files: vec!["bin/demo".into()],
            },
        );
        save_state(&paths, &state).unwrap();
        fs::write(command_shim_path(&paths, "demo"), b"shim").unwrap();

        remove(&paths, "demo", false).unwrap();
        assert!(!package.join("bin/demo").exists());
        assert_eq!(
            fs::read(package.join("settings.ini")).unwrap(),
            b"user setting"
        );
        assert!(load_state(&paths).unwrap().installed.is_empty());
        assert!(!command_shim_path(&paths, "demo").exists());
    }

    #[test]
    fn self_update_uses_a_new_release_directory() {
        let temp = tempdir().unwrap();
        let paths = paths(temp.path());
        initialize_home(&paths).unwrap();
        let mut package = PackageManifest {
            name: "mty".into(),
            entry: "bin/mty".into(),
            ..manifest(b"new")
        };
        package.files[0].path = "bin/mty".into();
        let source = temp.path().join("source");
        fs::create_dir_all(source.join("bin")).unwrap();
        fs::write(source.join("bin/mty"), b"new").unwrap();
        fs::create_dir_all(paths.tools.join("mty")).unwrap();
        fs::write(paths.tools.join("mty/mty.exe"), b"running old executable").unwrap();

        let installed = commit_install(&paths, &package, &source).unwrap();
        assert!(paths.tools.join("mty/mty.exe").exists());
        assert!(paths.tools.join("mty").join(installed.entry).exists());
    }
}

use std::process::{Command, Output};

fn cli(args: &[&str], home: &std::path::Path, no_color: bool) -> Output {
    let mut command = Command::new(env!("CARGO_BIN_EXE_mty"));
    command.args(args).env("MTY_HOME", home);
    for key in ["NO_COLOR", "CLICOLOR", "CLICOLOR_FORCE", "FORCE_COLOR"] {
        command.env_remove(key);
    }
    if no_color {
        command.env("NO_COLOR", "1");
    }
    command.output().unwrap()
}

fn has_color(bytes: &[u8]) -> bool {
    bytes.windows(2).any(|pair| pair == b"\x1b[")
}

#[test]
fn text_colors_preserve_tables_and_cover_errors() {
    let home = tempfile::tempdir().unwrap();
    std::fs::write(home.path().join("state.json"), r#"{"installed":{"demo":{"name":"demo","version":"1.0.0","platform":"windows","arch":"x86_64","entry":"bin/demo","files":[]}}}"#).unwrap();
    let plain = cli(&["--color", "never", "list"], home.path(), false);
    assert!(plain.status.success());
    assert!(!has_color(&plain.stdout));
    let colored = cli(&["list", "--color=always"], home.path(), true);
    assert!(colored.status.success());
    assert!(has_color(&colored.stdout));
    let mut stripped = anstream::AutoStream::new(Vec::new(), anstream::ColorChoice::Never);
    use std::io::Write;
    stripped.write_all(&colored.stdout).unwrap();
    assert_eq!(stripped.into_inner(), plain.stdout);
    for no_color in [false, true] {
        let auto = cli(&["list"], home.path(), no_color);
        assert!(auto.status.success());
        assert_eq!(auto.stdout, plain.stdout);
    }
    for args in [
        vec!["--color", "always", "remove", "missing"],
        vec!["--color", "always", "outdated", "missing"],
        vec!["--color", "always", "--unknown"],
        vec![
            "--color",
            "always",
            "install",
            "demo",
            "--version",
            "1.0",
            "--help",
        ],
    ] {
        let result = cli(&args, home.path(), false);
        assert!(
            has_color(&result.stdout) || has_color(&result.stderr),
            "{args:?}: {result:?}"
        );
        assert!(!String::from_utf8_lossy(&result.stderr).contains("panicked"));
    }
    let help = cli(&["--help", "--color", "never"], home.path(), false);
    assert!(help.status.success());
    assert!(!has_color(&help.stdout));
    assert!(String::from_utf8_lossy(&help.stdout).contains("--color"));
    let help = cli(
        &["--color", "always", "help", "install"],
        home.path(),
        false,
    );
    assert!(help.status.success());
    assert!(has_color(&help.stdout));
    for args in [
        vec!["--color", "never", "--unknown"],
        vec!["--json", "--color", "always", "--unknown"],
    ] {
        let result = cli(&args, home.path(), false);
        assert!(!result.status.success());
        assert!(!has_color(&result.stderr));
    }
    let system = cli(&["--color", "always", "system-info"], home.path(), false);
    assert!(system.status.success());
    assert!(has_color(&system.stdout));
    let search = cli(
        &[
            "--color",
            "always",
            "search",
            "demo",
            "--registry",
            "http://127.0.0.1:0",
        ],
        home.path(),
        false,
    );
    assert!(!search.status.success());
    assert!(has_color(&search.stdout));
    assert!(has_color(&search.stderr));
}

#[test]
fn json_diagnostics_and_completion_are_always_plain() {
    let home = tempfile::tempdir().unwrap();
    for args in [
        vec!["--color", "always", "--json", "list"],
        vec!["--color", "always", "--json", "outdated", "missing"],
        vec![
            "--color",
            "always",
            "--json",
            "search",
            "demo",
            "--registry",
            "http://127.0.0.1:0",
        ],
        vec!["--color", "always", "--json", "--help"],
    ] {
        let result = cli(&args, home.path(), false);
        assert!(!has_color(&result.stdout), "{args:?}");
        assert!(!has_color(&result.stderr), "{args:?}");
        if !args.contains(&"--help") {
            let json = if result.stdout.is_empty() {
                // stderr can contain progress before the final JSON error.
                result
                    .stderr
                    .split(|byte| *byte == b'\n')
                    .filter(|line| !line.is_empty())
                    .next_back()
                    .unwrap()
            } else {
                &result.stdout
            };
            // Search fails with one JSON error; outdated emits a result array on stdout.
            serde_json::from_slice::<serde_json::Value>(json).unwrap_or_else(|error| {
                panic!("{args:?}: {error}: {}", String::from_utf8_lossy(json))
            });
        }
    }
    for shell in ["bash", "zsh", "fish", "powershell"] {
        let plain = cli(
            &["--color", "never", "completion", shell],
            home.path(),
            false,
        );
        let colored = cli(
            &["--color", "always", "completion", shell],
            home.path(),
            false,
        );
        assert!(plain.status.success() && colored.status.success());
        assert_eq!(plain.stdout, colored.stdout);
        assert!(!has_color(&colored.stdout));
        let json = cli(
            &["--json", "--color", "always", "completion", shell],
            home.path(),
            false,
        );
        assert!(json.status.success());
        let value: serde_json::Value = serde_json::from_slice(&json.stdout).unwrap();
        assert_eq!(value["script"].as_str().unwrap().as_bytes(), plain.stdout);
    }
}

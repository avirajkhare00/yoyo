use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::Path;
use std::process::Command;

use tempfile::tempdir;

fn write_executable(path: &Path, contents: &str) {
    fs::write(path, contents).unwrap();
    let mut permissions = fs::metadata(path).unwrap().permissions();
    permissions.set_mode(0o755);
    fs::set_permissions(path, permissions).unwrap();
}

#[test]
fn install_helper_creates_temp_tap_without_git() {
    let tempdir = tempdir().unwrap();
    let bin_dir = tempdir.path().join("bin");
    let prefix_dir = tempdir.path().join("prefix");
    let prefix_bin_dir = prefix_dir.join("bin");
    let tap_repo = tempdir.path().join("tap-repo");
    let brew_log = tempdir.path().join("brew.log");
    let formula_path = tempdir.path().join("yoyo.rb");
    let script_path =
        Path::new(env!("CARGO_MANIFEST_DIR")).join(".github/scripts/install-homebrew-formula.sh");

    fs::create_dir_all(&bin_dir).unwrap();
    fs::create_dir_all(&prefix_bin_dir).unwrap();
    fs::create_dir_all(&tap_repo).unwrap();
    fs::write(&formula_path, "class Yoyo < Formula\nend\n").unwrap();

    write_executable(
        &bin_dir.join("brew"),
        r#"#!/usr/bin/env bash
set -euo pipefail

printf '%s\n' "$*" >> "$BREW_LOG"

case "${1:-}" in
  uninstall|untap|install|test)
    exit 0
    ;;
  tap-new)
    if [[ "${2:-}" != "--no-git" ]]; then
      echo "expected --no-git" >&2
      exit 1
    fi
    mkdir -p "$BREW_TAP_REPO/Formula"
    exit 0
    ;;
  --repository)
    printf '%s\n' "$BREW_TAP_REPO"
    exit 0
    ;;
  --prefix)
    printf '%s\n' "$BREW_PREFIX"
    exit 0
    ;;
  *)
    echo "unexpected brew invocation: $*" >&2
    exit 1
    ;;
esac
"#,
    );

    write_executable(
        &prefix_bin_dir.join("yoyo"),
        "#!/usr/bin/env bash\nset -euo pipefail\necho 'yoyo 1.14.2'\n",
    );

    let output = Command::new("bash")
        .arg(script_path)
        .arg(&formula_path)
        .env("BREW_LOG", &brew_log)
        .env("BREW_PREFIX", &prefix_dir)
        .env("BREW_TAP_REPO", &tap_repo)
        .env(
            "PATH",
            format!(
                "{}:{}",
                bin_dir.display(),
                std::env::var("PATH").unwrap_or_default()
            ),
        )
        .output()
        .unwrap();

    assert!(
        output.status.success(),
        "stdout:\n{}\n\nstderr:\n{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );

    let brew_log = fs::read_to_string(&brew_log).unwrap();
    assert!(brew_log.contains("tap-new --no-git yoyo/local-ci"));
    assert!(brew_log.contains("install yoyo/local-ci/yoyo"));
    assert!(brew_log.contains("test yoyo/local-ci/yoyo"));

    let copied_formula = fs::read_to_string(tap_repo.join("Formula/yoyo.rb")).unwrap();
    assert!(copied_formula.contains("class Yoyo < Formula"));
}

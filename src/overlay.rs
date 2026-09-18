//! RakuOS `rum` usr-overlay integration.
//!
//! RakuOS mounts a persistent overlayfs over `/usr`; `rum install` writes
//! packages into its upperdir and records their names in
//! `/var/lib/rakuos/packages.list`. Those packages stay in the overlay, live —
//! they are not baked into the image. dcli-bootc makes the set declarative:
//! `overlay import` snapshots the overlay into a module, and build/update (or
//! `overlay sync`) reconcile the live overlay to match it, installing added
//! packages and removing dropped ones with rum. Packages the base image
//! provides are kept rather than removed — see
//! `commands::overlay::base_provided_packages`.

use anyhow::{Context, Result};
use std::path::{Path, PathBuf};
use std::process::Command;

const PACKAGES_LIST: &str = "/var/lib/rakuos/packages.list";
const RPM_PACKAGES_LIST: &str = "/var/lib/rakuos/packages-rpm.list";

/// True when this host has the RakuOS rum overlay (list file + rum binary).
pub fn available() -> bool {
    Path::new(PACKAGES_LIST).exists() && which("rum").is_some()
}

/// Packages explicitly installed into the host overlay, sorted and deduped.
pub fn installed_packages() -> Result<Vec<String>> {
    let raw = std::fs::read_to_string(PACKAGES_LIST)
        .with_context(|| format!("Failed to read {}", PACKAGES_LIST))?;
    Ok(parse_list(&raw))
}

/// Packages installed into the overlay from local .rpm files. These cannot be
/// reproduced from `packages:` (no repo provides them) — the user must declare
/// them as `local_rpms`/`remote_rpms` instead.
pub fn installed_local_rpms() -> Vec<String> {
    std::fs::read_to_string(RPM_PACKAGES_LIST)
        .map(|raw| parse_list(&raw))
        .unwrap_or_default()
}

fn parse_list(raw: &str) -> Vec<String> {
    let mut pkgs: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    pkgs.sort();
    pkgs.dedup();
    pkgs
}

/// Remove packages from the host overlay with `rum remove`.
pub fn remove_packages(pkgs: &[String], dry_run: bool) -> Result<()> {
    run_rum("remove", pkgs, dry_run)
}

/// Install packages into the host overlay with `rum install`.
pub fn install_packages(pkgs: &[String], dry_run: bool) -> Result<()> {
    run_rum("install", pkgs, dry_run)
}

fn run_rum(action: &str, pkgs: &[String], dry_run: bool) -> Result<()> {
    if pkgs.is_empty() {
        return Ok(());
    }
    let mut cmd = Command::new("sudo");
    cmd.arg("rum").arg(action).arg("-y");
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.args(pkgs);
    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute 'rum {}'", action))?;
    if !status.success() {
        anyhow::bail!("rum {} failed", action);
    }
    Ok(())
}

/// Path rum keeps a staged copy of a locally-installed RPM at. rum names the
/// copy after the package (`<name>.rpm`); this is what `rakuos reset-overlay
/// --soft` and `rakuos-overlay-sync` reinstall from on a later boot.
pub fn staged_local_rpm(name: &str) -> PathBuf {
    Path::new("/var/lib/rakuos/local-rpms").join(format!("{}.rpm", name))
}

/// Install a local `.rpm` file into the host overlay with `rum install`. rum
/// copies the file under `/var/lib/rakuos/local-rpms` and records it in
/// `packages-rpm.list`, so the overlay can reinstall it after a reset.
pub fn install_local_rpm(path: &Path, dry_run: bool) -> Result<()> {
    let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
    let mut cmd = Command::new("sudo");
    cmd.arg("rum").arg("install").arg("-y");
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.arg(&abs);
    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute 'rum install {}'", abs.display()))?;
    if !status.success() {
        anyhow::bail!("rum install {} failed", abs.display());
    }
    Ok(())
}

/// Overwrite rum's staged copy of a local RPM with the project's file (root
/// owned). Repairs the case where a soft reset blanked the staged copy, so the
/// next overlay rebuild can still reinstall it.
pub fn stage_local_rpm(src: &Path, name: &str) -> Result<()> {
    let dst = staged_local_rpm(name);
    let src_str = src.to_string_lossy().to_string();
    let dst_str = dst.to_string_lossy().to_string();
    run_sudo(&[
        "install",
        "-D",
        "-m",
        "0644",
        "-o",
        "root",
        "-g",
        "root",
        src_str.as_str(),
        dst_str.as_str(),
    ])
    .with_context(|| format!("Failed to stage {} for the overlay", src.display()))
}

/// Overwrite the host overlay's package lists (root-owned) to match the given
/// contents, backing the originals up as `.bak` first.
///
/// `rum` itself never removes a package the base image provides — doing so
/// would whiteout the base copy — and `rakuos reset-overlay --soft` preserves
/// these lists. Rewriting them and then resetting is what actually reclaims
/// the redundant overlay entries; see `commands::overlay::clean`.
pub fn write_package_lists(packages: &[String], local_rpms: &[String]) -> Result<()> {
    write_root_list(PACKAGES_LIST, packages)?;
    write_root_list(RPM_PACKAGES_LIST, local_rpms)?;
    Ok(())
}

fn write_root_list(path: &str, entries: &[String]) -> Result<()> {
    let body: String = entries.iter().map(|e| format!("{}\n", e)).collect();

    if Path::new(path).exists() {
        let backup = format!("{}.bak", path);
        run_sudo(&["cp", "-a", path, &backup])
            .with_context(|| format!("Failed to back up {}", path))?;
    }

    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("list");
    let tmp = std::env::temp_dir().join(format!("dcli-bootc-{}-{}", std::process::id(), name));
    std::fs::write(&tmp, body).with_context(|| format!("Failed to stage {}", tmp.display()))?;

    let tmp_str = tmp.to_string_lossy().to_string();
    let result = run_sudo(&[
        "install",
        "-m",
        "0644",
        "-o",
        "root",
        "-g",
        "root",
        tmp_str.as_str(),
        path,
    ]);
    let _ = std::fs::remove_file(&tmp);
    result.with_context(|| format!("Failed to write {}", path))?;
    Ok(())
}

fn run_sudo(args: &[&str]) -> Result<()> {
    let status = Command::new("sudo")
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute 'sudo {}'", args.join(" ")))?;
    if !status.success() {
        anyhow::bail!("sudo {} failed", args.join(" "));
    }
    Ok(())
}

/// Schedule a clean rebuild of the usr-overlay on next boot. `--soft` keeps
/// `packages.list` and the local RPMs, so the overlay is reinstalled from the
/// (freshly rewritten) lists.
pub fn schedule_soft_reset() -> Result<()> {
    let status = Command::new("sudo")
        .args(["rakuos", "reset-overlay", "--soft"])
        .status()
        .context(
            "Failed to execute 'sudo rakuos reset-overlay --soft' (is 'rakuos' installed and on PATH?)",
        )?;
    if !status.success() {
        anyhow::bail!("rakuos reset-overlay --soft failed");
    }
    Ok(())
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(bin))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_skipping_comments_and_blanks() {
        let got = parse_list("steam\n\n# comment\nkitty\nsteam \n");
        assert_eq!(got, vec!["kitty", "steam"]);
    }
}

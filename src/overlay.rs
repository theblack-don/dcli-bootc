//! RakuOS `rum` usr-overlay integration.
//!
//! RakuOS mounts a persistent overlayfs over `/usr`; `rum install` writes
//! packages into its upperdir and records their names in
//! `/var/lib/rakuos/packages.list`. Those packages stay in the overlay, live —
//! they are not baked into the image. dcli-bootc makes the set declarative:
//! `overlay import` snapshots the overlay into a module, and build/update (or
//! `overlay sync`) reconcile the live overlay to match it, installing added
//! packages and removing dropped ones with rum.

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

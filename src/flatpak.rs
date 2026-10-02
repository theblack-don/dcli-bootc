//! Live user-scoped Flatpak management.
//!
//! Every flatpak a module declares — `flatpaks:`/`user_flatpaks:`,
//! `remove_flatpaks:`/`user_remove_flatpaks:`, and
//! `flatpak_remotes:`/`user_flatpak_remotes:` — is installed into the invoking
//! user's flatpak installation (`~/.local/share/flatpak`) and reconciled on the
//! running system. Flatpaks are **never** baked into the image: a system
//! install (`/var/lib/flatpak`) lives in machine-local state under bootc and
//! would not survive a rebase, so it is not supported.
//!
//! These run as the invoking user (no sudo) so `flatpak --user` targets the
//! right home directory.

use anyhow::{Context, Result};
use colored::*;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::config::{self, FlatpakRemote, Profile};
use crate::module::{self, ResolvedModule};

/// True when a `flatpak` binary is available on this host.
pub fn available() -> bool {
    which("flatpak").is_some()
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(bin))
            .find(|p| p.is_file())
    })
}

/// Apps installed in the invoking user's flatpak installation.
pub fn installed_apps() -> Result<Vec<String>> {
    let out = Command::new("flatpak")
        .args(["list", "--user", "--app", "--columns=application"])
        .output()
        .context("Failed to run 'flatpak list --user'")?;
    if !out.status.success() {
        anyhow::bail!("flatpak list --user failed");
    }
    Ok(sorted_lines(&out.stdout))
}

/// Remote names configured for the invoking user.
pub fn user_remotes() -> Result<Vec<String>> {
    let out = Command::new("flatpak")
        .args(["remotes", "--user", "--columns=name"])
        .output()
        .context("Failed to run 'flatpak remotes --user'")?;
    if !out.status.success() {
        anyhow::bail!("flatpak remotes --user failed");
    }
    Ok(sorted_lines(&out.stdout))
}

fn sorted_lines(bytes: &[u8]) -> Vec<String> {
    let mut v: Vec<String> = String::from_utf8_lossy(bytes)
        .lines()
        .map(|l| l.trim().to_string())
        .filter(|l| !l.is_empty())
        .collect();
    v.sort();
    v.dedup();
    v
}

fn run(args: &[String]) -> Result<()> {
    if args.is_empty() {
        return Ok(());
    }
    let status = Command::new("flatpak")
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute 'flatpak {}'", args.join(" ")))?;
    if !status.success() {
        anyhow::bail!("flatpak {} failed", args.join(" "));
    }
    Ok(())
}

/// Declared flatpak app IDs across the given modules (deduped, sorted).
///
/// `flatpaks:` and `user_flatpaks:` are equivalent — both are user-scoped.
fn collect_apps(modules: &[ResolvedModule]) -> Vec<String> {
    let mut apps: Vec<String> = modules
        .iter()
        .flat_map(|rm| rm.module.flatpaks.iter().chain(rm.module.user_flatpaks.iter()))
        .cloned()
        .collect();
    apps.sort();
    apps.dedup();
    apps
}

/// Declared flatpak removals across the given modules (deduped, sorted).
///
/// `remove_flatpaks:` and `user_remove_flatpaks:` are equivalent — both are
/// user-scoped.
fn collect_removals(modules: &[ResolvedModule]) -> Vec<String> {
    let mut apps: Vec<String> = modules
        .iter()
        .flat_map(|rm| {
            rm.module
                .remove_flatpaks
                .iter()
                .chain(rm.module.user_remove_flatpaks.iter())
        })
        .cloned()
        .collect();
    apps.sort();
    apps.dedup();
    apps
}

/// Declared flatpak remotes across the given modules (deduped by name, order
/// preserved; `flatpak_remotes:` and `user_flatpak_remotes:` are equivalent).
fn collect_remotes(modules: &[ResolvedModule]) -> Vec<FlatpakRemote> {
    crate::module::collect_flatpak_remotes(modules)
}

fn modules(project_dir: &Path, profile: &Profile) -> Result<Vec<ResolvedModule>> {
    module::load_enabled_modules(project_dir, &profile.enabled_modules)
}

/// Reconcile the invoking user's flatpak installation with the flatpaks
/// declared across enabled modules. Returns the number of changes made.
///
/// Declared apps are (re)applied with `flatpak install --or-update`, which
/// installs missing apps and updates installed ones, so running this during
/// `build`/`update` keeps them current. No-op when flatpak is absent or no
/// module declares any user flatpaks.
pub fn sync(
    project_dir: &Path,
    profile: &Profile,
    force: bool,
    dry_run: bool,
    announce: bool,
) -> Result<usize> {
    if !available() {
        return Ok(0);
    }

    let resolved = modules(project_dir, profile)?;
    let declared = collect_apps(&resolved);
    let removals = collect_removals(&resolved);
    let remotes = collect_remotes(&resolved);

    if declared.is_empty() && removals.is_empty() {
        if announce {
            println!("{} No user_flatpaks declared — nothing to sync.", "!".yellow());
        }
        return Ok(0);
    }

    let installed = installed_apps()?;
    let existing_remotes = user_remotes().unwrap_or_default();

    let missing_remotes: Vec<&FlatpakRemote> = remotes
        .iter()
        .filter(|r| !existing_remotes.contains(&r.name))
        .collect();
    let to_apply = declared.clone();
    let to_remove: Vec<String> = removals
        .iter()
        .filter(|a| installed.contains(a))
        .cloned()
        .collect();

    println!();
    println!("{} User flatpaks (flatpak --user):", "▶".cyan());
    for r in &missing_remotes {
        println!("    {} add remote {} ({})", "+".green(), r.name, r.url);
    }
    for a in &to_apply {
        println!("    {} install/update {}", "+".green(), a);
    }
    for a in &to_remove {
        println!("    {} remove {}", "-".yellow(), a);
    }

    if dry_run {
        println!("  (dry run — nothing changed)");
        return Ok(0);
    }

    if !force && !crate::bootc::confirm("Apply these changes to your user flatpak installation?") {
        println!("  Skipped — run 'dcli-bootc flatpak sync' when ready.");
        return Ok(0);
    }

    for r in &missing_remotes {
        run(&[
            "remote-add".to_string(),
            "--if-not-exists".to_string(),
            "--user".to_string(),
            r.name.clone(),
            r.url.clone(),
        ])?;
    }
    // Install one app per invocation. `flatpak install` resolves every ref in
    // a single call against the remote that provides the first ref, so batching
    // apps from different remotes (e.g. Flathub + a vendor remote) fails with
    // "Nothing matches <app> in remote <first-remote>".
    for app in &to_apply {
        run(&[
            "install".to_string(),
            "-y".to_string(),
            "--or-update".to_string(),
            "--user".to_string(),
            app.clone(),
        ])?;
    }
    if !to_remove.is_empty() {
        let mut args = vec!["uninstall".to_string(), "-y".to_string(), "--user".to_string()];
        args.extend(to_remove.iter().cloned());
        run(&args)?;
    }

    println!(
        "{} User flatpaks synced ({} app(s), {} remote(s){})",
        "✓".green(),
        to_apply.len(),
        missing_remotes.len(),
        if to_remove.is_empty() {
            String::new()
        } else {
            format!(", {} removed", to_remove.len())
        }
    );
    Ok(to_apply.len() + missing_remotes.len() + to_remove.len())
}

/// Show drift between declared `user_flatpaks:` and the live user installation.
pub fn status(project_dir: &Path, profile: &Profile) -> Result<()> {
    if !available() {
        println!("{} flatpak is not available on this host.", "!".yellow());
        return Ok(());
    }

    let resolved = modules(project_dir, profile)?;
    let declared = collect_apps(&resolved);
    let remotes = collect_remotes(&resolved);
    let installed = installed_apps()?;
    let existing_remotes = user_remotes().unwrap_or_default();

    println!("{} User flatpaks (flatpak --user)", "▶".cyan());
    if declared.is_empty() && remotes.is_empty() {
        println!("  No user_flatpaks declared.");
        return Ok(());
    }

    for a in &declared {
        if installed.contains(a) {
            println!("    {} {}", "✓".green(), a);
        } else {
            println!("    {} {} (declared, not installed)", "+".cyan(), a);
        }
    }
    let extra: Vec<&String> = installed.iter().filter(|a| !declared.contains(*a)).collect();
    for a in &extra {
        println!("    {} {} (installed, not declared)", "-".yellow(), a);
    }
    for r in &remotes {
        if existing_remotes.contains(&r.name) {
            println!("    {} remote {}", "✓".green(), r.name);
        } else {
            println!("    {} remote {} (missing)", "+".cyan(), r.name);
        }
    }
    Ok(())
}

/// `dcli-bootc flatpak sync` — the on-demand form of [`sync`].
pub fn sync_command(project_dir: &Path, force: bool, dry_run: bool, host: Option<&str>) -> Result<()> {
    let (profile, _) = config::resolve_effective_profile(project_dir, host)?;
    sync(project_dir, &profile, force, dry_run, true)?;
    Ok(())
}

/// `dcli-bootc flatpak status`.
pub fn status_command(project_dir: &Path, host: Option<&str>) -> Result<()> {
    let (profile, _) = config::resolve_effective_profile(project_dir, host)?;
    status(project_dir, &profile)
}

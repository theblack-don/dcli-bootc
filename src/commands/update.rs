use anyhow::{Context, Result};
use colored::*;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::bootc;
use crate::commands::build;
use crate::module;
use crate::plan;

/// Update = pull the latest base image, rebuild the custom image on top of
/// it, and (after confirmation) switch the deployment to the fresh build.
pub fn run(
    project_dir: &Path,
    tag: Option<String>,
    force: bool,
    auto_reboot: bool,
    base_image_sel: Option<String>,
    cache: bool,
    no_import: bool,
    no_switch: bool,
    host: Option<String>,
) -> Result<()> {
    let prepared = build::PreparedBuild::prepare(project_dir, tag, base_image_sel, host)?;
    run_prepared(
        project_dir,
        &prepared,
        force,
        auto_reboot,
        cache,
        no_import,
        no_switch,
    )
}

pub(super) fn run_prepared(
    project_dir: &Path,
    prepared: &build::PreparedBuild,
    force: bool,
    auto_reboot: bool,
    cache: bool,
    no_import: bool,
    no_switch: bool,
) -> Result<()> {
    let profile = &prepared.profile;
    let active_host = &prepared.active_host;
    let base_image = &prepared.base_image;
    let image_ref = &prepared.image_ref;

    println!("{} Updating base image {}", "▶".cyan(), base_image);
    if let Some(h) = active_host {
        println!("  Host: {}", h);
    }
    println!();

    // ── Show what the rebuild will do before pulling anything ──
    // (the prepared build skips its own plan output when invoked from here)
    let modules = module::load_enabled_modules(project_dir, &profile.enabled_modules)?;
    plan::print_build_plan(
        project_dir,
        &modules,
        image_ref,
        base_image,
        profile.package_manager,
        profile.kernel.as_ref(),
    );

    // ── Pull the latest base image ──
    let base_before = bootc::local_image_digest(base_image);
    println!("  Pulling {} ...", base_image);
    let status = Command::new("sudo")
        .arg("podman")
        .arg("pull")
        .arg(base_image)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit())
        .status()
        .context("Failed to pull base image")?;

    if !status.success() {
        eprintln!("{} Pull failed", "✗".red());
        std::process::exit(1);
    }

    println!("{} Pulled {}", "✓".green(), base_image);
    match (&base_before, &bootc::local_image_digest(base_image)) {
        (Some(before), Some(after)) if before != after => println!(
            "  Base image updated: {} → {}",
            bootc::short_digest(before),
            bootc::short_digest(after)
        ),
        (Some(_), Some(_)) => println!("  {}", "Base image unchanged (already up to date)".dimmed()),
        _ => {}
    }
    println!();

    // ── Rebuild the custom image on the fresh base, then switch ──
    // (skip the freshness check — we just pulled the base image; upgrade the
    // overlay's packages too, so one `update` refreshes image + overlay)
    build::run_prepared(
        project_dir,
        prepared,
        build::BuildOptions {
            auto_reboot,
            force,
            no_cache: !cache,
            no_import,
            no_switch,
            skip_freshness_check: true,
            upgrade_overlay: true,
        },
    )
}

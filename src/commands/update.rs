use anyhow::{Context, Result};
use colored::*;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::bootc;
use crate::commands::build;
use crate::config;
use crate::module;
use crate::plan;
use crate::project;

/// Update = pull the latest base image, rebuild the custom image on top of
/// it, and (after confirmation) switch the deployment to the fresh build.
pub fn run(
    project_dir: &Path,
    tag: Option<String>,
    force: bool,
    auto_reboot: bool,
    nvidia: bool,
    cache: bool,
    no_import: bool,
    host: Option<String>,
) -> Result<()> {
    let (profile, active_host) = config::resolve_effective_profile(project_dir, host.as_deref())?;

    let base_image = if nvidia {
        profile
            .nvidia_base_image
            .as_deref()
            .unwrap_or(&profile.base_image)
    } else {
        &profile.base_image
    };

    println!("{} Updating base image {}", "▶".cyan(), base_image);
    if let Some(h) = &active_host {
        println!("  Host: {}", h);
    }
    println!();

    // ── Show what the rebuild will do before pulling anything ──
    // (build::run skips its own plan output when invoked from here)
    // Explicit --tag wins; otherwise a host build is tagged with the host name
    let image_tag = tag
        .clone()
        .or_else(|| active_host.clone())
        .unwrap_or_else(|| profile.default_tag.clone());
    let image_name = project::get_project_name(project_dir)?;
    let image_ref = format!("localhost/{}:{}", image_name, image_tag);
    let modules = module::load_enabled_modules(project_dir, &profile.enabled_modules)?;
    plan::print_build_plan(
        project_dir,
        &modules,
        &image_ref,
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
    build::run(project_dir, tag, nvidia, auto_reboot, force, !cache, no_import, true, true, host)
}

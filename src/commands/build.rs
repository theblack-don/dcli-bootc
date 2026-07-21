use anyhow::{Context, Result};
use colored::*;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::bootc;
use crate::commands::update;
use crate::config;
use crate::generator;
use crate::hash;
use crate::module;
use crate::plan;
use crate::project;
use crate::registry::{self, Freshness};

pub fn run(
    project_dir: &Path,
    tag: Option<String>,
    nvidia: bool,
    auto_reboot: bool,
    force: bool,
    no_cache: bool,
    skip_freshness_check: bool,
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

    // Explicit --tag wins; otherwise a host build is tagged with the host name
    let image_tag = tag
        .or_else(|| active_host.clone())
        .unwrap_or_else(|| profile.default_tag.clone());
    let image_name = project::get_project_name(project_dir)?;
    let image_ref = format!("localhost/{}:{}", image_name, image_tag);

    // ── Make sure Containerfile/build.sh match the current modules + host ──
    sync_generated(project_dir, &profile, active_host.as_deref())?;

    println!(
        "{} Building {} from {}",
        "▶".cyan(),
        image_ref,
        base_image
    );
    println!();

    // ── Warn when the registry has a newer base image ──
    if !skip_freshness_check
        && handoff_to_update_if_stale(
            project_dir,
            base_image,
            &image_tag,
            nvidia,
            auto_reboot,
            force,
            active_host.clone(),
        )?
    {
        return Ok(());
    }

    // ── Show what the build will do before doing any work ──
    // (skipped when called from `update`, which already printed the plan
    // before pulling the base image)
    if !skip_freshness_check {
        let modules = module::load_enabled_modules(project_dir, &profile.enabled_modules)?;
        plan::print_build_plan(project_dir, &modules, &image_ref, base_image);
    }

    // ── Cache-busting content hash of the build inputs ──
    let build_hash = hash::compute_build_hash(project_dir)?;
    println!("  Build input hash: {}", &build_hash[..12]);
    if no_cache {
        println!("  Layer cache disabled (--no-cache)");
    }
    println!();

    // ── podman build (inherits stdin so sudo can prompt) ──
    let mut cmd = Command::new("sudo");
    cmd.arg("podman")
        .arg("build")
        .arg("--build-arg")
        .arg(format!("BASE_IMAGE={}", base_image))
        .arg("--build-arg")
        .arg(format!("DCLI_BUILD_HASH={}", build_hash))
        .arg("-t")
        .arg(format!("{}:{}", image_name, image_tag))
        .arg(project_dir)
        .stdin(Stdio::inherit())
        .stdout(Stdio::inherit())
        .stderr(Stdio::inherit());
    if no_cache {
        cmd.arg("--no-cache");
    }
    let status = cmd.status().context("Failed to execute podman build")?;

    if !status.success() {
        eprintln!("{} Build failed", "✗".red());
        std::process::exit(1);
    }

    println!();
    println!("{} Build complete", "✓".green());

    // ── Show what was built, and whether it differs from the booted deployment ──
    let built_digest = bootc::local_image_digest(&image_ref);
    if let Some(digest) = &built_digest {
        println!("  Image digest: {}", bootc::short_digest(digest));
    }

    // ── Verify the image actually contains what the modules declare ──
    verify_packages(project_dir, &profile, &image_ref)?;

    if let (Some(built), Ok(bootc_status)) = (&built_digest, bootc::status()) {
        if let Some(booted) = &bootc_status.booted_digest {
            if booted == built {
                println!(
                    "{} This image is identical to the currently booted deployment.",
                    "!".yellow()
                );
            }
        }
    }

    // ── Bootc switch prompt ──
    println!();
    if force || bootc::confirm("Switch bootc deployment to this image?") {
        let staged = bootc::switch_to_local(&image_ref)?;
        bootc::maybe_reboot(auto_reboot, staged);
    } else {
        println!();
        println!("  Switch later with: dcli-bootc switch --tag {}", image_tag);
    }

    Ok(())
}

/// Warn when the local copy of the base image is older than the registry's
/// and offer to hand off to `update` (pull the latest base + rebuild fresh)
/// instead. Returns true when the update flow took over and the caller
/// should stop.
fn handoff_to_update_if_stale(
    project_dir: &Path,
    base_image: &str,
    image_tag: &str,
    nvidia: bool,
    auto_reboot: bool,
    force: bool,
    host: Option<String>,
) -> Result<bool> {
    match registry::check_base_freshness(base_image) {
        Freshness::Fresh => {
            println!("  {}", "Base image is current".dimmed());
            println!();
            Ok(false)
        }
        Freshness::Unknown => Ok(false),
        Freshness::Stale { local, remote } => {
            println!(
                "{} A newer base image is available for {}",
                "!".yellow(),
                base_image
            );
            println!("    local:    {}", bootc::short_digest(&local));
            println!("    registry: {}", bootc::short_digest(&remote));
            println!("    This build would use the outdated local copy.");
            println!();

            if force {
                println!("  Run 'dcli-bootc update' to rebuild on the latest base.");
                println!();
                return Ok(false);
            }

            if bootc::confirm("Pull the latest base and rebuild fresh (dcli-bootc update) instead?")
            {
                println!();
                update::run(
                    project_dir,
                    Some(image_tag.to_string()),
                    force,
                    auto_reboot,
                    nvidia,
                    false,
                    host,
                )?;
                return Ok(true);
            }

            println!("  Continuing with the local base image — update later with: dcli-bootc update");
            println!();
            Ok(false)
        }
    }
}

/// Regenerate Containerfile/build.sh when they drift from the modules or the
/// active host, so a build always reflects the current declarative
/// configuration.
fn sync_generated(project_dir: &Path, profile: &config::Profile, host: Option<&str>) -> Result<()> {
    let generated = generator::generate(project_dir, profile, host)?;

    let containerfile_path = project_dir.join("Containerfile");
    let build_sh_path = project_dir.join("build_files").join("build.sh");

    let current_cf = std::fs::read_to_string(&containerfile_path).unwrap_or_default();
    let current_sh = std::fs::read_to_string(&build_sh_path).unwrap_or_default();

    if current_cf == generated.containerfile && current_sh == generated.build_sh {
        return Ok(());
    }

    println!(
        "{} Generated files were out of sync with modules/host — regenerating",
        "➜".yellow()
    );

    std::fs::create_dir_all(project_dir.join("build_files"))?;
    std::fs::write(&containerfile_path, &generated.containerfile)
        .with_context(|| format!("Failed to write {}", containerfile_path.display()))?;
    std::fs::write(&build_sh_path, &generated.build_sh)
        .with_context(|| format!("Failed to write {}", build_sh_path.display()))?;

    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(&build_sh_path, std::fs::Permissions::from_mode(0o755))?;

    Ok(())
}

/// Verify that every package declared in the modules is actually present in
/// the built image, and that every removed package is actually gone. Aborts
/// before the switch prompt if the image does not match the configuration.
fn verify_packages(project_dir: &Path, profile: &config::Profile, image_ref: &str) -> Result<()> {
    let modules = module::load_enabled_modules(project_dir, &profile.enabled_modules)?;

    let mut wanted: Vec<String> = Vec::new();
    let mut unwanted: Vec<String> = Vec::new();
    for rm in &modules {
        wanted.extend(rm.module.packages.iter().cloned());
        unwanted.extend(rm.module.remove.iter().cloned());
        // Local RPMs are declared by filename — resolve their real package
        // names from the RPM headers so they are verified too.
        for entry in &rm.module.local_rpms {
            match plan::local_rpm_package_name(project_dir, entry) {
                Some(name) => wanted.push(name),
                None => println!(
                    "  {} Could not read package name from rpms/{} — skipping its verification",
                    "!".yellow(),
                    entry
                ),
            }
        }
    }
    if wanted.is_empty() && unwanted.is_empty() {
        return Ok(());
    }

    println!();
    println!("{} Verifying packages in the built image...", "▶".cyan());

    let output = Command::new("sudo")
        .args([
            "podman",
            "run",
            "--rm",
            image_ref,
            "rpm",
            "-qa",
            "--queryformat",
            "%{NAME}\\n",
        ])
        .output()
        .context("Failed to inspect the built image")?;

    if !output.status.success() {
        println!(
            "{} Could not verify image contents (rpm query failed) — skipping check",
            "!".yellow()
        );
        return Ok(());
    }

    let installed: std::collections::HashSet<String> = String::from_utf8_lossy(&output.stdout)
        .lines()
        .map(|line| line.trim().to_string())
        .collect();

    let mut problems = Vec::new();
    for pkg in &wanted {
        if !installed.contains(pkg) {
            problems.push(format!("missing from image: {}", pkg));
        }
    }
    for pkg in &unwanted {
        if installed.contains(pkg) {
            problems.push(format!("should have been removed: {}", pkg));
        }
    }

    if problems.is_empty() {
        println!(
            "{} Verified: {} package(s) present, {} removal(s) confirmed",
            "✓".green(),
            wanted.len(),
            unwanted.len()
        );
        Ok(())
    } else {
        eprintln!("{} Image does not match your modules:", "✗".red());
        for problem in &problems {
            eprintln!("    - {}", problem);
        }
        eprintln!();
        eprintln!("  Fix the modules/repos and rebuild. Not offering to switch to this image.");
        std::process::exit(1);
    }
}

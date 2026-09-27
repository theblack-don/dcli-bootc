use anyhow::{Context, Result};
use colored::*;
use std::path::Path;
use std::process::{Command, Stdio};

use crate::bootc;
use crate::commands::update;
use crate::config;
use crate::dotfiles;
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
    upgrade_overlay: bool,
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
        plan::print_build_plan(
            project_dir,
            &modules,
            &image_ref,
            base_image,
            profile.package_manager,
            profile.kernel.as_ref(),
        );
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

    // ── Symlink dotfiles into the invoking user's home (host-side; the
    // container build cannot touch home directories). Failures are reported
    // but do not block the switch prompt — dotfiles are orthogonal to the
    // OS image.
    if !profile.dotfiles.is_empty() {
        println!();
        if let Err(e) = dotfiles::apply(project_dir, &profile.dotfiles) {
            eprintln!("{} Failed to apply dotfiles: {:#}", "!".yellow(), e);
        }
    }

    // ── Reconcile the host usr-overlay with the modules: packages deleted
    // from a module are uninstalled from the host overlay. No-op on hosts
    // without a usable overlay backend or projects without overlay_module.
    if let Err(e) = crate::commands::overlay::sync(project_dir, &profile, force, false, false) {
        eprintln!("{} Overlay sync failed: {:#}", "!".yellow(), e);
    }

    // ── On `update`, also refresh the packages already installed in the
    // overlay: sync only adds/removes to match the manifest, so without this
    // they'd stay at their old versions. No-op on hosts without a usable
    // overlay backend or projects without overlay_module.
    if upgrade_overlay {
        if let Err(e) = crate::commands::overlay::upgrade(&profile, force, true) {
            eprintln!("{} Overlay upgrade failed: {:#}", "!".yellow(), e);
        }
    }

    // ── Reconcile the invoking user's flatpak installation with the modules'
    // `user_flatpaks:` (host-side; user-scoped apps are never baked into the
    // image). No-op when flatpak is absent or nothing is declared.
    if let Err(e) = crate::flatpak::sync(project_dir, &profile, force, false, false) {
        eprintln!("{} User flatpak sync failed: {:#}", "!".yellow(), e);
    }

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
    let mut wanted_flatpaks: Vec<String> = Vec::new();
    let mut unwanted_flatpaks: Vec<String> = Vec::new();
    let mut wanted_appimages: Vec<String> = Vec::new();
    for rm in &modules {
        wanted.extend(rm.module.packages.iter().cloned());
        unwanted.extend(rm.module.remove.iter().cloned());
        wanted_flatpaks.extend(rm.module.flatpaks.iter().cloned());
        unwanted_flatpaks.extend(rm.module.remove_flatpaks.iter().cloned());
        wanted_appimages.extend(rm.module.appimages.iter().cloned());
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
    if let Some(kernel) = &profile.kernel {
        wanted.extend(kernel.packages.iter().cloned());
        unwanted.extend(kernel.effective_remove());
    }
    if wanted.is_empty()
        && unwanted.is_empty()
        && wanted_flatpaks.is_empty()
        && unwanted_flatpaks.is_empty()
        && wanted_appimages.is_empty()
    {
        return Ok(());
    }

    println!();
    println!("{} Verifying packages in the built image...", "▶".cyan());

    let (query_cmd, query_args) = match profile.package_manager {
        config::PackageManager::Dnf | config::PackageManager::Zypper => {
            ("rpm", vec!["-qa", "--queryformat", "%{NAME}\\n"])
        }
        config::PackageManager::Apt => {
            ("dpkg-query", vec!["-W", "-f=${Package}\\n"])
        }
        config::PackageManager::Pacman => {
            ("pacman", vec!["-Q", "--quiet"])
        }
    };

    let mut cmd = Command::new("sudo");
    cmd.arg("podman")
        .arg("run")
        .arg("--rm")
        .arg(image_ref)
        .arg(query_cmd);
    cmd.args(&query_args);

    let output = cmd.output().context("Failed to inspect the built image")?;

    if !output.status.success() {
        println!(
            "{} Could not verify image contents ({} failed) — skipping check",
            "!".yellow(),
            query_cmd
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

    // ── Kernel checks: exactly one kernel, vmlinuz + initramfs where bootc
    // expects them, and the pinned version when one was declared ──
    if let Some(kernel) = &profile.kernel {
        let mut cmd = Command::new("sudo");
        cmd.arg("podman")
            .arg("run")
            .arg("--rm")
            .arg(image_ref)
            .arg("sh")
            .arg("-c")
            .arg(
                "n=$(ls -d /usr/lib/modules/*/ 2>/dev/null | wc -l); \
                 echo \"kernel-count:$n\"; \
                 kver=$(basename /usr/lib/modules/*/ 2>/dev/null | head -n1); \
                 [ -f \"/usr/lib/modules/$kver/vmlinuz\" ] && echo vmlinuz:yes || echo vmlinuz:no; \
                 [ -f \"/usr/lib/modules/$kver/initramfs.img\" ] && echo initramfs:yes || echo initramfs:no",
            );

        match cmd.output() {
            Ok(output) if output.status.success() => {
                let report = String::from_utf8_lossy(&output.stdout);
                let field = |key: &str| -> Option<String> {
                    let prefix = format!("{}:", key);
                    report
                        .lines()
                        .find_map(|l| l.strip_prefix(prefix.as_str()))
                        .map(|s| s.trim().to_string())
                };

                match field("kernel-count").as_deref() {
                    Some("1") => {}
                    Some(other) => problems.push(format!(
                        "expected exactly one kernel in /usr/lib/modules, found {}",
                        other
                    )),
                    None => {}
                }
                if field("vmlinuz").as_deref() != Some("yes") {
                    problems
                        .push("no vmlinuz found under /usr/lib/modules/<kver>".to_string());
                }
                if kernel.regen_initramfs && field("initramfs").as_deref() != Some("yes") {
                    problems.push(
                        "no initramfs.img under /usr/lib/modules/<kver> — the kernel may not boot"
                            .to_string(),
                    );
                }
            }
            _ => println!(
                "  {} Could not verify the kernel layout in the image — skipping check",
                "!".yellow()
            ),
        }

        if let Some(version) = kernel.version.as_ref().filter(|v| !v.is_empty()) {
            for pkg in &kernel.packages {
                let output = Command::new("sudo")
                    .arg("podman")
                    .arg("run")
                    .arg("--rm")
                    .arg(image_ref)
                    .arg("rpm")
                    .arg("-q")
                    .arg("--qf")
                    .arg("%{VERSION}-%{RELEASE}")
                    .arg(pkg)
                    .output();
                if let Ok(output) = output {
                    let got = String::from_utf8_lossy(&output.stdout).trim().to_string();
                    if output.status.success() && !got.starts_with(version.as_str()) {
                        problems.push(format!(
                            "kernel package {} is {} but kernel.version pins {}",
                            pkg, got, version
                        ));
                    }
                }
            }
        }
    }

    // ── Flatpak verification (best-effort: skipped when the image has no flatpak) ──
    let mut wanted_flatpak_seen: std::collections::HashSet<String> = std::collections::HashSet::new();
    if !wanted_flatpaks.is_empty() || !unwanted_flatpaks.is_empty() {
        let mut cmd = Command::new("sudo");
        cmd.arg("podman")
            .arg("run")
            .arg("--rm")
            .arg(image_ref)
            .arg("flatpak")
            .args(["list", "--system", "--app", "--columns=application"]);

        let output = cmd.output();
        match output {
            Ok(output) if output.status.success() => {
                let flatpak_installed: std::collections::HashSet<String> =
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .map(|line| line.trim().to_string())
                        .collect();

                for app in wanted_flatpaks
                    .iter()
                    .filter(|a| wanted_flatpak_seen.insert(a.to_string()))
                {
                    if !flatpak_installed.contains(app) {
                        problems.push(format!("flatpak missing from image: {}", app));
                    }
                }
                for app in &unwanted_flatpaks {
                    if flatpak_installed.contains(app) {
                        problems.push(format!("flatpak should have been removed: {}", app));
                    }
                }
            }
            Ok(_) => println!(
                "  {} Could not verify flatpaks (flatpak not available in this image) — skipping check",
                "!".yellow()
            ),
            Err(e) => println!(
                "  {} Could not verify flatpaks: {} — skipping check",
                "!".yellow(),
                e
            ),
        }
    }

    // ── AppImage verification ──
    if !wanted_appimages.is_empty() {
        let mut cmd = Command::new("sudo");
        cmd.arg("podman")
            .arg("run")
            .arg("--rm")
            .arg(image_ref)
            .arg("ls")
            .arg("/usr/local/bin");

        let output = cmd.output();
        match output {
            Ok(output) if output.status.success() => {
                let installed: std::collections::HashSet<String> =
                    String::from_utf8_lossy(&output.stdout)
                        .lines()
                        .map(|line| line.trim().to_string())
                        .collect();

                for entry in &wanted_appimages {
                    let bin_name = std::path::Path::new(entry)
                        .file_name()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| entry.clone());
                    if !installed.contains(&bin_name) {
                        problems.push(format!(
                            "appimage missing from /usr/local/bin: {}",
                            bin_name
                        ));
                    }
                }
            }
            Ok(_) => println!(
                "  {} Could not verify appimages (could not list /usr/local/bin in the image) — skipping check",
                "!".yellow()
            ),
            Err(e) => println!(
                "  {} Could not verify appimages: {} — skipping check",
                "!".yellow(),
                e
            ),
        }
    }

    if problems.is_empty() {
        println!(
            "{} Verified: {} package(s) present, {} removal(s) confirmed{}",
            "✓".green(),
            wanted.len(),
            unwanted.len(),
            if !wanted_flatpaks.is_empty()
                || !unwanted_flatpaks.is_empty()
                || !wanted_appimages.is_empty()
            {
                format!(
                    ", {} flatpak(s) present, {} flatpak removal(s) confirmed, {} appimage(s) installed",
                    wanted_flatpaks.len(),
                    unwanted_flatpaks.len(),
                    wanted_appimages.len()
                )
            } else {
                String::new()
            }
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

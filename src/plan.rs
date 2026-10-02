//! Pre-build plan output: what a build will install, remove, download, and
//! enable — plus a best-effort comparison against the previously built image
//! so users can see what is new between builds.

use colored::*;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::bootc;
use crate::config;
use crate::module::ResolvedModule;

/// Print everything the build will do, then (best-effort) what changed
/// relative to the previously built image.
pub fn print_build_plan(
    project_dir: &Path,
    modules: &[ResolvedModule],
    image_ref: &str,
    base_image: &str,
    package_manager: config::PackageManager,
    kernel: Option<&config::KernelConfig>,
) {
    println!(
        "{} Build plan: {} (base: {})",
        "▶".cyan(),
        image_ref.bold(),
        base_image
    );
    println!();

    if let Some(kernel) = kernel {
        print_kernel_plan(kernel);
    }

    for rm in modules {
        print_module_plan(project_dir, rm);
    }

    println!();
    print_comparison(project_dir, modules, image_ref, package_manager, kernel);

    println!();
    println!(
        "  {}",
        format!("Package versions are resolved by {} during the build.", package_manager.as_str()).dimmed()
    );
    println!();
}

/// The kernel section's contribution to the build, as indented lines under a
/// header. Skipped when nothing is configured.
fn print_kernel_plan(kernel: &config::KernelConfig) {
    let mut lines: Vec<String> = Vec::new();

    if !kernel.repo.import_keys.is_empty() {
        lines.push(format!("import keys: {}", kernel.repo.import_keys.join(", ")));
    }
    if !kernel.repo.copr.is_empty() {
        lines.push(format!("copr enable: {}", kernel.repo.copr.join(", ")));
    }
    for repo in &kernel.repo.rpm_urls {
        lines.push(format!("repo '{}': {}", repo.name, repo.baseurl));
    }
    if !kernel.repo.enable.is_empty() {
        lines.push(format!("enable repo: {}", kernel.repo.enable.join(", ")));
    }
    let remove = kernel.effective_remove();
    if !remove.is_empty() {
        lines.push(format!("remove stock ({}): {}", remove.len(), remove.join(", ")));
    }
    if !kernel.packages.is_empty() {
        lines.push(format!(
            "install ({}): {}",
            kernel.packages.len(),
            kernel.install_specs().join(", ")
        ));
    }
    if kernel.versionlock {
        lines.push("versionlock: yes".to_string());
    }
    if kernel.regen_initramfs && (!kernel.packages.is_empty() || !remove.is_empty()) {
        lines.push("regenerate initramfs + depmod".to_string());
    }

    if lines.is_empty() {
        return;
    }
    println!("  {}:", "kernel".bold());
    for line in lines {
        println!("    {}", line);
    }
}

/// One module's contribution to the build, as indented lines under a header.
/// Modules that contribute nothing are skipped.
fn print_module_plan(project_dir: &Path, rm: &ResolvedModule) {
    let m = &rm.module;
    let mut lines: Vec<String> = Vec::new();

    if !m.repos.import_keys.is_empty() {
        lines.push(format!("import keys: {}", m.repos.import_keys.join(", ")));
    }
    if !m.repos.copr.is_empty() {
        lines.push(format!("copr enable: {}", m.repos.copr.join(", ")));
    }
    for repo in &m.repos.rpm_urls {
        lines.push(format!("repo '{}': {}", repo.name, repo.baseurl));
    }
    if !m.repos.enable.is_empty() {
        lines.push(format!("enable repo: {}", m.repos.enable.join(", ")));
    }
    if !m.remove.is_empty() {
        lines.push(format!("remove ({}): {}", m.remove.len(), m.remove.join(", ")));
    }
    if !m.packages.is_empty() {
        lines.push(format!(
            "install ({}): {}",
            m.packages.len(),
            m.packages.join(", ")
        ));
    }
    if !m.groups.is_empty() {
        lines.push(format!(
            "install group ({}): {}",
            m.groups.len(),
            m.groups.join(", ")
        ));
    }
    for rpm in &m.remote_rpms {
        let filename = rpm.filename.clone().unwrap_or_else(|| {
            rpm.url
                .rsplit('/')
                .next()
                .unwrap_or("package.rpm")
                .to_string()
        });
        lines.push(format!("download + install: {} ({})", filename, rpm.url));
    }
    for entry in &m.local_rpms {
        let size = std::fs::metadata(project_dir.join("rpms").join(entry))
            .map(|md| format_size(md.len()))
            .unwrap_or_else(|_| "missing file!".to_string());
        match local_rpm_package_name(project_dir, entry) {
            Some(name) => lines.push(format!("local rpm: {} [{}, {}]", name, entry, size)),
            None => lines.push(format!("local rpm: {} [{}]", entry, size)),
        }
    }
    for entry in &m.appimages {
        let size = std::fs::metadata(project_dir.join("appimages").join(entry))
            .map(|md| format_size(md.len()))
            .unwrap_or_else(|_| "missing file!".to_string());
        lines.push(format!(
            "appimage -> /usr/local/bin/{} [{}]",
            entry, size
        ));
    }
    if !m.files.is_empty() {
        lines.push(format!("files overlay: {}", m.files.join(", ")));
    }
    for remote in m.flatpak_remotes.iter().chain(m.user_flatpak_remotes.iter()) {
        lines.push(format!(
            "user flatpak remote: {} ({})",
            remote.name, remote.url
        ));
    }
    let flatpak_installs: Vec<&String> = m
        .flatpaks
        .iter()
        .chain(m.user_flatpaks.iter())
        .collect();
    if !flatpak_installs.is_empty() {
        lines.push(format!(
            "user flatpak install ({}): {} (host-side, flatpak --user)",
            flatpak_installs.len(),
            flatpak_installs
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    let flatpak_removals: Vec<&String> = m
        .remove_flatpaks
        .iter()
        .chain(m.user_remove_flatpaks.iter())
        .collect();
    if !flatpak_removals.is_empty() {
        lines.push(format!(
            "user flatpak remove ({}): {}",
            flatpak_removals.len(),
            flatpak_removals
                .iter()
                .map(|a| a.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }
    if m.scripts.pre.is_some() {
        lines.push("script: pre".to_string());
    }
    if m.scripts.post.is_some() {
        lines.push("script: post".to_string());
    }
    if !m.services.is_empty() {
        lines.push(format!("enable service: {}", m.services.join(", ")));
    }
    for fb in &m.first_boot_services {
        lines.push(format!(
            "first-boot service: {} (ConditionPathExists={})",
            fb.name, fb.condition_path
        ));
    }

    if lines.is_empty() {
        return;
    }
    println!("  {}:", rm.name.bold());
    for line in lines {
        println!("    {}", line);
    }
}

/// Best-effort diff against the previously built image: which declared
/// packages are new since then, and which declared removals are actually
/// present there. Silently degrades when the image or sudo is unavailable.
fn print_comparison(
    project_dir: &Path,
    modules: &[ResolvedModule],
    image_ref: &str,
    package_manager: config::PackageManager,
    kernel: Option<&config::KernelConfig>,
) {
    let Some(digest) = bootc::local_image_digest(image_ref) else {
        println!(
            "  {}",
            "No previous build of this image found to compare against.".dimmed()
        );
        return;
    };

    let Some(installed) = query_image_packages(image_ref, package_manager) else {
        println!(
            "  {}",
            "(could not read the previous build's package list — skipping comparison)".dimmed()
        );
        return;
    };

    let mut wanted: Vec<String> = Vec::new();
    let mut removals: Vec<String> = Vec::new();
    for rm in modules {
        wanted.extend(rm.module.packages.iter().cloned());
        removals.extend(rm.module.remove.iter().cloned());
        for entry in &rm.module.local_rpms {
            if let Some(name) = local_rpm_package_name(project_dir, entry) {
                wanted.push(name);
            }
        }
    }
    if let Some(kernel) = kernel {
        wanted.extend(kernel.packages.iter().cloned());
        removals.extend(kernel.effective_remove());
    }

    let new: Vec<&String> = wanted.iter().filter(|p| !installed.contains(*p)).collect();
    let to_remove: Vec<&String> = removals.iter().filter(|p| installed.contains(*p)).collect();

    println!(
        "  Compared with previous build ({}):",
        bootc::short_digest(&digest)
    );
    if new.is_empty() {
        println!("    new packages: none — everything declared is already installed");
    } else {
        println!("    new packages ({}): {}", new.len(), join_refs(&new));
    }
    println!("    already installed: {}", wanted.len() - new.len());
    if !to_remove.is_empty() {
        println!("    will be removed: {}", join_refs(&to_remove));
    } else if !removals.is_empty() {
        println!("    removals: already absent from the previous build");
    }
}

fn join_refs(items: &[&String]) -> String {
    items
        .iter()
        .map(|s| s.as_str())
        .collect::<Vec<_>>()
        .join(", ")
}

/// Package names installed in an image, queried by running the appropriate
/// package manager query inside it.
fn query_image_packages(
    image_ref: &str,
    package_manager: config::PackageManager,
) -> Option<HashSet<String>> {
    let (query_cmd, query_args) = match package_manager {
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

    let output = cmd.output().ok()?;
    if !output.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&output.stdout)
            .lines()
            .map(|line| line.trim().to_string())
            .collect(),
    )
}

/// Query the package name of a local RPM file from its header (host-side).
pub fn local_rpm_package_name(project_dir: &Path, entry: &str) -> Option<String> {
    let path = project_dir.join("rpms").join(entry);
    let output = Command::new("rpm")
        .args(["-qp", "--queryformat", "%{NAME}"])
        .arg(&path)
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let name = String::from_utf8_lossy(&output.stdout).trim().to_string();
    if name.is_empty() {
        None
    } else {
        Some(name)
    }
}

fn format_size(bytes: u64) -> String {
    const MIB: u64 = 1024 * 1024;
    if bytes >= MIB {
        format!("{} MB", bytes / MIB)
    } else {
        format!("{} KB", bytes / 1024)
    }
}

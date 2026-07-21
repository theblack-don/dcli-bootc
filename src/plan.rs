//! Pre-build plan output: what a build will install, remove, download, and
//! enable — plus a best-effort comparison against the previously built image
//! so users can see what is new between builds.

use colored::*;
use std::collections::HashSet;
use std::path::Path;
use std::process::Command;

use crate::bootc;
use crate::module::ResolvedModule;

/// Print everything the build will do, then (best-effort) what changed
/// relative to the previously built image.
pub fn print_build_plan(
    project_dir: &Path,
    modules: &[ResolvedModule],
    image_ref: &str,
    base_image: &str,
) {
    println!(
        "{} Build plan: {} (base: {})",
        "▶".cyan(),
        image_ref.bold(),
        base_image
    );
    println!();

    for rm in modules {
        print_module_plan(project_dir, rm);
    }

    println!();
    print_comparison(project_dir, modules, image_ref);

    println!();
    println!(
        "  {}",
        "Package versions are resolved by dnf during the build.".dimmed()
    );
    println!();
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
    if !m.files.is_empty() {
        lines.push(format!("files overlay: {}", m.files.join(", ")));
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
fn print_comparison(project_dir: &Path, modules: &[ResolvedModule], image_ref: &str) {
    let Some(digest) = bootc::local_image_digest(image_ref) else {
        println!(
            "  {}",
            "No previous build of this image found to compare against.".dimmed()
        );
        return;
    };

    let Some(installed) = query_image_packages(image_ref) else {
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

/// Package names installed in an image, queried by running rpm inside it.
fn query_image_packages(image_ref: &str) -> Option<HashSet<String>> {
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
        .ok()?;
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

use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::config::{KernelConfig, OverlayBackend, PackageManager, Profile, RepoConfig};
use crate::module::{collect_flatpak_remotes, load_enabled_modules, ResolvedModule};

/// Package-manager-specific shell commands
struct PkgManager {
    pub install: &'static str,
    pub remove: &'static str,
    pub clean: &'static str,
}

impl PkgManager {
    fn for_pm(pm: PackageManager) -> Self {
        match pm {
            PackageManager::Dnf => PkgManager {
                install: "dnf install -y",
                remove: "dnf remove -y",
                clean: "dnf clean all && rm -rf /var/cache/dnf/*",
            },
            PackageManager::Apt => PkgManager {
                install: "apt-get install -y",
                remove: "apt-get remove -y",
                clean: "apt-get clean && rm -rf /var/lib/apt/lists/*",
            },
            PackageManager::Zypper => PkgManager {
                install: "zypper install -y",
                remove: "zypper remove -y",
                clean: "zypper clean && rm -rf /var/cache/zypp/*",
            },
            PackageManager::Pacman => PkgManager {
                install: "pacman -S --noconfirm",
                remove: "pacman -Rns --noconfirm",
                clean: "pacman -Scc --noconfirm",
            },
        }
    }
}

/// Generated output files
pub struct GeneratedFiles {
    pub containerfile: String,
    pub build_sh: String,
}

/// Generate Containerfile and build.sh from a profile + its modules.
///
/// `host` is recorded in the build.sh header so that switching hosts changes
/// the generated output — which both triggers drift regeneration in `build`
/// and busts the podman layer cache via the build-input hash.
pub fn generate(project_dir: &Path, profile: &Profile, host: Option<&str>) -> Result<GeneratedFiles> {
    if let Some(om) = &profile.overlay_module {
        if profile.enabled_modules.contains(om) {
            eprintln!(
                "Warning: overlay_module '{}' is in enabled_modules — its packages are installed into the live usr-overlay, not baked into the image. Remove it from enabled_modules to avoid baking them too.",
                om
            );
        }
    }

    if let Some(kernel) = &profile.kernel {
        validate_kernel(profile, kernel)?;
    }

    let modules = load_enabled_modules(project_dir, &profile.enabled_modules)?;

    validate_local_rpms(project_dir, &modules)?;
    validate_local_appimages(project_dir, &modules)?;
    crate::dotfiles::validate(project_dir, &profile.dotfiles)?;

    let has_local_rpms = modules.iter().any(|rm| !rm.module.local_rpms.is_empty());
    let has_local_appimages = modules.iter().any(|rm| !rm.module.appimages.is_empty());
    let containerfile = generate_containerfile(profile, has_local_rpms, has_local_appimages);
    let build_sh = generate_build_sh(profile, &modules, host);

    Ok(GeneratedFiles {
        containerfile,
        build_sh,
    })
}

/// Ensure every declared local RPM is a plain relative filename and exists in
/// the project's rpms/ directory — fail at gen time instead of mid-build.
fn validate_local_rpms(project_dir: &Path, modules: &[ResolvedModule]) -> Result<()> {
    for rm in modules {
        for entry in &rm.module.local_rpms {
            let rel = Path::new(entry);
            if rel.is_absolute()
                || rel
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                anyhow::bail!(
                    "Module '{}' declares local_rpms entry '{}' — entries must be filenames relative to the project's rpms/ directory",
                    rm.name,
                    entry
                );
            }
            let path = project_dir.join("rpms").join(entry);
            if !path.is_file() {
                anyhow::bail!(
                    "Module '{}' declares local_rpms entry '{}' but '{}' does not exist — put the .rpm file in the project's rpms/ directory",
                    rm.name,
                    entry,
                    path.display()
                );
            }
        }
    }
    Ok(())
}

/// Ensure every declared local AppImage is a plain relative filename and
/// exists in the project's appimages/ directory — fail at gen time instead of
/// mid-build.
fn validate_local_appimages(project_dir: &Path, modules: &[ResolvedModule]) -> Result<()> {
    for rm in modules {
        for entry in &rm.module.appimages {
            let rel = Path::new(entry);
            if rel.is_absolute()
                || rel
                    .components()
                    .any(|c| matches!(c, std::path::Component::ParentDir))
            {
                anyhow::bail!(
                    "Module '{}' declares appimages entry '{}' — entries must be filenames relative to the project's appimages/ directory",
                    rm.name,
                    entry
                );
            }
            let path = project_dir.join("appimages").join(entry);
            if !path.is_file() {
                anyhow::bail!(
                    "Module '{}' declares appimages entry '{}' but '{}' does not exist — put the AppImage file in the project's appimages/ directory",
                    rm.name,
                    entry,
                    path.display()
                );
            }
        }
    }
    Ok(())
}

fn generate_containerfile(profile: &Profile, has_local_rpms: bool, has_local_appimages: bool) -> String {
    // COPY of a missing directory fails the build, so these are only copied
    // when a module actually declares local files.
    let copy_rpms = if has_local_rpms { "COPY rpms /rpms\n" } else { "" };
    let copy_appimages = if has_local_appimages {
        "COPY appimages /appimages\n"
    } else {
        ""
    };
    format!(
        r#"ARG BASE_IMAGE={0}

FROM scratch AS ctx
COPY build_files /
COPY files /files
{1}{2}
FROM ${{BASE_IMAGE}}

# Cache-bust: dcli-bootc passes a content hash of build_files/ and files/ as
# DCLI_BUILD_HASH. Bind-mounted content is not part of podman's layer cache
# key, so without this the customization layer could be silently reused even
# after the build inputs change.
ARG DCLI_BUILD_HASH=dev

RUN --mount=type=bind,from=ctx,source=/,target=/ctx \
    --mount=type=cache,dst=/var/cache \
    --mount=type=tmpfs,dst=/tmp \
    /bin/sh -c 'echo "dcli-bootc build inputs: ${{DCLI_BUILD_HASH}}" && exec /ctx/build.sh'

RUN bootc container lint
"#,
        profile.base_image, copy_rpms, copy_appimages
    )
}

fn generate_build_sh(profile: &Profile, modules: &[ResolvedModule], host: Option<&str>) -> String {
    let mut lines = Vec::new();
    let pm = PkgManager::for_pm(profile.package_manager);

    lines.push("#!/usr/bin/env bash".to_string());
    lines.push("set -euo pipefail".to_string());
    lines.push(String::new());
    lines.push(format!("# Generated by dcli-bootc gen — edit modules/*.yaml instead"));
    lines.push(format!("# Profile: {}", profile.name));
    if let Some(h) = host {
        lines.push(format!("# Host: {}", h));
    }
    lines.push(String::new());

    // ── Flatpak remotes (added once, before any module's installs) ──
    let flatpak_remotes = collect_flatpak_remotes(modules);
    if !flatpak_remotes.is_empty() {
        lines.push("# === flatpak remotes ===".to_string());
        for remote in &flatpak_remotes {
            lines.push(format!(
                "flatpak remote-add --if-not-exists --system {} \"{}\"",
                remote.name, remote.url
            ));
        }
    }

    // ── Kernel management (foundational: before any module section) ──
    if let Some(kernel) = &profile.kernel {
        emit_kernel_section(&mut lines, kernel, profile.package_manager);
    }

    // Apps already emitted, so the same flatpak is not installed/removed
    // twice across modules.
    let mut installed_flatpaks: HashSet<String> = HashSet::new();
    let mut removed_flatpaks: HashSet<String> = HashSet::new();

    for rm in modules {
        let m = &rm.module;

        // Skip empty modules with only a note
        let has_repos = !m.repos.copr.is_empty()
            || !m.repos.rpm_urls.is_empty()
            || !m.repos.enable.is_empty()
            || !m.repos.import_keys.is_empty()
            || !m.repos.apt_lines.is_empty();
        let has_remove = !m.remove.is_empty();
        let has_install = !m.packages.is_empty()
            || !m.groups.is_empty()
            || !m.remote_rpms.is_empty()
            || !m.local_rpms.is_empty()
            || !m.appimages.is_empty();
        let has_files = !m.files.is_empty();
        let has_scripts = m.scripts.pre.is_some() || m.scripts.post.is_some();
        let has_services = !m.services.is_empty() || !m.first_boot_services.is_empty();
        let has_flatpaks = !m.flatpaks.is_empty() || !m.remove_flatpaks.is_empty();

        if !has_repos
            && !has_remove
            && !has_install
            && !has_files
            && !has_scripts
            && !has_services
            && !has_flatpaks
        {
            continue;
        }

        lines.push(String::new());
        lines.push(format!("# === {}: {} ===", rm.name, m.description));

        // ── Pre script ──
        if let Some(script) = &m.scripts.pre {
            lines.push(String::new());
            lines.push(format!("# pre script ({})", rm.name));
            for line in script.trim().lines() {
                lines.push(line.to_string());
            }
        }

        // ── Repos ──
        emit_repos(&mut lines, &m.repos, profile.package_manager);

        // ── Remove packages ──
        if !m.remove.is_empty() {
            let pkgs = m.remove.join(" \\\n    ");
            lines.push(String::new());
            lines.push(format!("{} \\\n    {}", pm.remove, pkgs));
        }

        // ── Install packages ──
        if !m.packages.is_empty() {
            let pkgs = m.packages.join(" \\\n    ");
            lines.push(String::new());
            lines.push(format!("{} \\\n    {}", pm.install, pkgs));
        }

        // ── Install dnf package groups ──
        if !m.groups.is_empty() && matches!(profile.package_manager, PackageManager::Dnf) {
            let groups = m.groups.join(" \\\n    ");
            lines.push(String::new());
            lines.push(format!("dnf group install -y \\\n    {}", groups));
        }

        // ── Remote RPMs ──
        for rpm in &m.remote_rpms {
            let filename = rpm
                .filename
                .clone()
                .unwrap_or_else(|| {
                    rpm.url
                        .rsplit('/')
                        .next()
                        .unwrap_or("package.rpm")
                        .to_string()
                });
            lines.push(format!(
                "curl -L -o /tmp/{} \"{}\"",
                filename, rpm.url
            ));
        }

        if !m.remote_rpms.is_empty() {
            let rpm_files: Vec<String> = m
                .remote_rpms
                .iter()
                .map(|r| {
                    r.filename
                        .clone()
                        .unwrap_or_else(|| {
                            r.url.rsplit('/').next().unwrap_or("package.rpm").to_string()
                        })
                })
                .map(|f| format!("/tmp/{}", f))
                .collect();
            lines.push(format!("{} \\\n    {}", pm.install, rpm_files.join(" \\\n    ")));
            lines.push("rm -f /tmp/*.rpm".to_string());
        }

        // ── Local RPMs (from the project's rpms/ directory, via the ctx mount) ──
        if !m.local_rpms.is_empty() {
            let rpm_files: Vec<String> = m
                .local_rpms
                .iter()
                .map(|f| format!("/ctx/rpms/{}", f))
                .collect();
            lines.push(String::new());
            lines.push(format!(
                "{} \\\n    {}",
                pm.install,
                rpm_files.join(" \\\n    ")
            ));
        }

        // ── AppImages (from the project's appimages/ directory, via the ctx mount) ──
        for entry in &m.appimages {
            let bin_name = Path::new(entry)
                .file_name()
                .map(|s| s.to_string_lossy().to_string())
                .unwrap_or_else(|| entry.clone());
            lines.push(String::new());
            lines.push(format!("# AppImage: {}", bin_name));
            lines.push(format!(
                "install -Dm0755 /ctx/appimages/{} /usr/local/bin/{}",
                entry, bin_name
            ));
            lines.push("# Extract desktop integration (.desktop + icons)".to_string());
            lines.push("TMPDIR=$(mktemp -d)".to_string());
            lines.push("cd \"$TMPDIR\"".to_string());
            lines.push(format!(
                "/ctx/appimages/{} --appimage-extract >/dev/null 2>&1 && {{",
                entry
            ));
            lines.push("    desktop=$(find squashfs-root -maxdepth 2 -name '*.desktop' | head -n1)".to_string());
            lines.push("    [ -n \"$desktop\" ] && install -Dm0644 \"$desktop\" /usr/share/applications/".to_string());
            lines.push("    find squashfs-root -maxdepth 3 -type f \\( -name '*.png' -o -name '*.svg' -o -name '*.ico' \\) -exec install -Dm0644 {} /usr/share/icons/hicolor/ \\; 2>/dev/null || true".to_string());
            lines.push("}".to_string());
            lines.push("rm -rf \"$TMPDIR\"".to_string());
            lines.push("cd /".to_string());
        }

        // ── Flatpaks (system-wide into the image) ──
        let apps: Vec<String> = m
            .flatpaks
            .iter()
            .filter(|a| installed_flatpaks.insert(a.to_string()))
            .cloned()
            .collect();
        if !apps.is_empty() {
            // Installs come from the first configured remote (flathub by
            // default); declare flatpak_remotes to use another one.
            // --or-update: apps already in the base image update instead of erroring.
            let remote = &flatpak_remotes[0].name;
            lines.push(String::new());
            lines.push(format!(
                "flatpak install -y --or-update --system {} \\\n    {}",
                remote,
                apps.join(" \\\n    ")
            ));
        }

        let removed: Vec<String> = m
            .remove_flatpaks
            .iter()
            .filter(|a| removed_flatpaks.insert(a.to_string()))
            .cloned()
            .collect();
        if !removed.is_empty() {
            lines.push(String::new());
            lines.push(format!(
                "flatpak uninstall -y --system \\\n    {}",
                removed.join(" \\\n    ")
            ));
        }

        // ── Files overlay ──
        if !m.files.is_empty() {
            lines.push(String::new());
            lines.push("# Copy files/ tree into image".to_string());
            lines.push("rsync -rlptD /ctx/files/ /".to_string());
        }

        // ── Post script ──
        if let Some(script) = &m.scripts.post {
            lines.push(String::new());
            lines.push(format!("# post script ({})", rm.name));
            for line in script.trim().lines() {
                lines.push(line.to_string());
            }
        }

        // ── Services (always-on) ──
        // --force so a service that aliases a name the base image already
        // claims (e.g. display-manager.service) can take it over instead of
        // systemctl refusing to overwrite the existing symlink.
        for svc in &m.services {
            lines.push(format!("systemctl enable --force {}", svc));
        }

        // ── First-boot services (ConditionPathExists guard) ──
        for fb_svc in &m.first_boot_services {
            let unit_name = &fb_svc.name;
            let unit_path = format!("/etc/systemd/system/{}", unit_name);
            let cond_path = &fb_svc.condition_path;

            if let Some(content) = &fb_svc.unit_content {
                lines.push(format!(
                    r#"install -Dm0644 /dev/stdin {} << 'SVC'
{}"#,
                    unit_path, content
                ));
                lines.push("SVC".to_string());
            } else {
                lines.push(format!(
                    "# First-boot service: {} (guarded by ConditionPathExists={})",
                    unit_name, cond_path
                ));
            }
            lines.push(format!("systemctl enable --force {}", unit_name));
        }
    }

    // ── Baked base-package list (for overlay backends with a merged rpmdb) ──
    // Record the package set baked into this image so the host-side overlay
    // backend can tell base-provided packages from overlay-installed ones and
    // avoid overlayfs whiteouts. Skipped for the RakuOS rum backend, which has
    // a separate base rpmdb.
    if profile.overlay_module.is_some() && profile.overlay_backend != OverlayBackend::Rum {
        lines.push(String::new());
        lines.push("# === overlay base package list ===".to_string());
        lines.push("# Base-image package set, for usr-overlay whiteout safety.".to_string());
        lines.push(format!("mkdir -p {}", overlay_base_list_dir()));
        lines.push(format!(
            "rpm -qa --qf '%{{NAME}}\\n' | sort -u > {}",
            crate::overlay::BAKED_BASE_LIST
        ));
    }

    // ── Final cleanup ──
    lines.push(String::new());
    lines.push("# === cleanup ===".to_string());
    lines.push(format!("{}", pm.clean));
    lines.push(String::new());

    lines.join("\n")
}

/// Directory of the baked base-package list (`/usr/share/dcli-bootc`).
fn overlay_base_list_dir() -> &'static str {
    match crate::overlay::BAKED_BASE_LIST.rsplit_once('/') {
        Some((dir, _)) => dir,
        None => "/usr/share/dcli-bootc",
    }
}

/// Emit a module's or the kernel's repo setup (COPRs, rpm repos, key imports,
/// enabling shipped repos). Shared so kernel repos and module repos behave
/// identically.
fn emit_repos(lines: &mut Vec<String>, repos: &RepoConfig, pm: PackageManager) {
    match pm {
        PackageManager::Dnf | PackageManager::Zypper => {
            if !repos.import_keys.is_empty() {
                for key in &repos.import_keys {
                    lines.push(format!("rpmkeys --import \"{}\"", key));
                }
            }

            for copr in &repos.copr {
                lines.push(format!("dnf5 -y copr enable {}", copr));
            }

            for repo in &repos.rpm_urls {
                let name = &repo.name;
                let baseurl = &repo.baseurl;
                lines.push(format!(
                    r#"cat > /etc/yum.repos.d/{}.repo << 'REPO'
[{}]
name={}
baseurl={}
type=rpm-md
skip_if_unavailable=True
gpgcheck={}
repo_gpgcheck=0
enabled=1
enabled_metadata=1
REPO"#,
                    name,
                    name,
                    name,
                    baseurl,
                    if repo.gpgkey.is_some() { "1" } else { "0" },
                ));
                if let Some(gpgkey) = &repo.gpgkey {
                    lines.push(format!("rpmkeys --import \"{}\"", gpgkey));
                }
            }

            for repo_name in &repos.enable {
                lines.push(format!(
                    r#"sed -i 's/enabled=0/enabled=1/' /etc/yum.repos.d/{}.repo"#,
                    repo_name
                ));
            }
        }
        PackageManager::Apt => {
            for line in &repos.apt_lines {
                lines.push(format!(
                    "echo \"{}\" >> /etc/apt/sources.list",
                    line.replace("\"", "\\\"")
                ));
            }
            if !repos.import_keys.is_empty() {
                lines.push("apt-get update".to_string());
                for key in &repos.import_keys {
                    lines.push(format!("curl -fsSL \"{}\" | apt-key add -", key));
                }
            }
        }
        PackageManager::Pacman => {
            for repo in &repos.rpm_urls {
                let baseurl = &repo.baseurl;
                lines.push(format!(
                    r#"cat >> /etc/pacman.conf << 'REPO'
[{}]
Server = {}
REPO"#,
                    repo.name, baseurl
                ));
            }
            if !repos.import_keys.is_empty() {
                lines.push("pacman -Sy --noconfirm".to_string());
                for key in &repos.import_keys {
                    lines.push(format!(
                        "curl -fsSL \"{}\" | pacman-key --add - && pacman-key --lsign-key {}",
                        key, key
                    ));
                }
            }
        }
    }
}

/// Validate kernel management: dnf-only, and a version only applies to a
/// declared replacement package.
fn validate_kernel(profile: &Profile, kernel: &KernelConfig) -> Result<()> {
    if profile.package_manager != PackageManager::Dnf {
        anyhow::bail!(
            "profile '{}' declares kernel management but package_manager is '{}' — kernel management currently requires dnf",
            profile.name,
            profile.package_manager.as_str()
        );
    }
    if kernel
        .version
        .as_ref()
        .map_or(false, |v| !v.is_empty())
        && kernel.packages.is_empty()
    {
        anyhow::bail!(
            "profile '{}' sets kernel.version but kernel.packages is empty — a version only applies to declared kernel packages",
            profile.name
        );
    }
    if kernel.versionlock && kernel.packages.is_empty() {
        anyhow::bail!(
            "profile '{}' sets kernel.versionlock but kernel.packages is empty — declare the kernel package(s) to pin",
            profile.name
        );
    }
    Ok(())
}

/// Emit the kernel-management section: repo, stock removal, replacement
/// install, version pinning, and initramfs regeneration. Emitted before any
/// module section because a kernel swap is foundational.
fn emit_kernel_section(lines: &mut Vec<String>, kernel: &KernelConfig, pm: PackageManager) {
    let pkg = PkgManager::for_pm(pm);
    lines.push(String::new());
    lines.push("# === kernel ===".to_string());

    // ── Repo(s) providing the kernel ──
    emit_repos(lines, &kernel.repo, pm);

    // ── Remove the stock kernel(s) ──
    // Only remove packages that are actually installed, so the auto-default
    // Fedora list stays valid on base images that name things differently,
    // while a genuine dnf failure still aborts the build.
    let remove = kernel.effective_remove();
    if !remove.is_empty() {
        let quoted = remove
            .iter()
            .map(|p| format!("\"{}\"", p))
            .collect::<Vec<_>>()
            .join(" ");
        lines.push(String::new());
        lines.push("# Remove stock kernel(s) that are present".to_string());
        lines.push("__dcli_kernel_remove=()".to_string());
        lines.push(format!(
            "for __dcli_p in {}; do rpm -q \"$__dcli_p\" >/dev/null 2>&1 && __dcli_kernel_remove+=(\"$__dcli_p\"); done",
            quoted
        ));
        lines.push(format!(
            "if [ ${{#__dcli_kernel_remove[@]}} -gt 0 ]; then {} \"${{__dcli_kernel_remove[@]}}\"; fi",
            pkg.remove
        ));
    }

    // ── Install the replacement kernel(s) ──
    if !kernel.packages.is_empty() {
        let specs = kernel.install_specs();
        lines.push(String::new());
        lines.push(format!(
            "{} \\\n    {}",
            pkg.install,
            specs.join(" \\\n    ")
        ));
    }

    // ── Pin the installed version ──
    if kernel.versionlock {
        lines.push(String::new());
        lines.push("# Fail fast when the dnf versionlock plugin is unavailable".to_string());
        lines.push("if ! dnf versionlock list >/dev/null 2>&1; then".to_string());
        lines.push(
            "    echo 'error: kernel.versionlock requires the dnf versionlock plugin (install dnf-plugins-core)' >&2"
                .to_string(),
        );
        lines.push("    exit 1".to_string());
        lines.push("fi".to_string());
        lines.push(format!(
            "dnf versionlock add \\\n    {}",
            kernel.packages.join(" \\\n    ")
        ));
    }

    // ── Regenerate module deps + initramfs for the single kernel ──
    if kernel.regen_initramfs && (!kernel.packages.is_empty() || !remove.is_empty()) {
        lines.push(String::new());
        lines.push("# Regenerate modules.dep and the initramfs in /usr/lib/modules".to_string());
        lines.push("# (bootc requires exactly one kernel and an initramfs.img there)".to_string());
        lines.push("__dcli_kmods=(/usr/lib/modules/*/)".to_string());
        lines.push("if [ \"${#__dcli_kmods[@]}\" -ne 1 ]; then".to_string());
        lines.push(
            "    echo \"error: expected exactly one kernel in /usr/lib/modules, found ${#__dcli_kmods[@]}\" >&2"
                .to_string(),
        );
        lines.push("    ls -1 /usr/lib/modules >&2 || true".to_string());
        lines.push("    exit 1".to_string());
        lines.push("fi".to_string());
        lines.push("__dcli_kver=$(basename \"${__dcli_kmods[0]}\")".to_string());
        lines.push("depmod -a \"$__dcli_kver\"".to_string());
        lines.push(
            "dracut --force --no-hostonly --kver \"$__dcli_kver\" \"/usr/lib/modules/$__dcli_kver/initramfs.img\""
                .to_string(),
        );
        lines.push(
            "rm -f /boot/initramfs-*.img /boot/vmlinuz-* /boot/symvers-*.gz 2>/dev/null || true"
                .to_string(),
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn profile(overlay_module: Option<&str>, backend: OverlayBackend) -> Profile {
        Profile {
            name: "test".to_string(),
            base_image: "ghcr.io/ublue-os/bazzite:stable".to_string(),
            nvidia_base_image: None,
            default_tag: "latest".to_string(),
            enabled_modules: vec![],
            package_manager: PackageManager::Dnf,
            dotfiles: vec![],
            overlay_module: overlay_module.map(str::to_string),
            overlay_backend: backend,
            kernel: None,
        }
    }

    #[test]
    fn bakes_base_package_list_for_kinpkg_overlay() {
        let out = generate_build_sh(
            &profile(Some("overlay-packages"), OverlayBackend::Kinpkg),
            &[],
            None,
        );
        assert!(out.contains("mkdir -p /usr/share/dcli-bootc"), "{out}");
        assert!(
            out.contains("> /usr/share/dcli-bootc/base-packages.list"),
            "{out}"
        );
        assert!(out.contains("rpm -qa --qf '%{NAME}\\n'"), "{out}");
    }

    #[test]
    fn bakes_base_package_list_for_auto_overlay() {
        let out = generate_build_sh(&profile(Some("overlay"), OverlayBackend::Auto), &[], None);
        assert!(out.contains("base-packages.list"), "{out}");
    }

    #[test]
    fn skips_base_package_list_for_rum_or_no_overlay() {
        let rum = generate_build_sh(&profile(Some("overlay"), OverlayBackend::Rum), &[], None);
        assert!(!rum.contains("base-packages.list"), "{rum}");

        let none = generate_build_sh(&profile(None, OverlayBackend::Auto), &[], None);
        assert!(!none.contains("base-packages.list"), "{none}");
    }
}


use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

/// Supported package managers for build-time package operations
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum PackageManager {
    #[default]
    Dnf,
    Apt,
    Zypper,
    Pacman,
}

impl PackageManager {
    pub fn as_str(self) -> &'static str {
        match self {
            PackageManager::Dnf => "dnf",
            PackageManager::Apt => "apt",
            PackageManager::Zypper => "zypper",
            PackageManager::Pacman => "pacman",
        }
    }
}

/// Which usr-overlay implementation dcli-bootc reconciles against.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum OverlayBackend {
    /// Prefer RakuOS `rum` when present, otherwise kinpkg.
    #[default]
    Auto,
    /// RakuOS `rum` usr-overlay.
    Rum,
    /// kineticos persistent `/usr` overlay managed by `kinpkg`.
    Kinpkg,
}

/// Top-level profile for a bootc image project
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,

    #[serde(default = "default_base_image")]
    pub base_image: String,

    #[serde(default)]
    pub nvidia_base_image: Option<String>,

    #[serde(default = "default_tag")]
    pub default_tag: String,

    #[serde(default, alias = "modules")]
    pub enabled_modules: Vec<String>,

    /// Package manager used in the base image (dnf, apt, zypper, pacman).
    /// Auto-detected at build time if not specified; default is "dnf".
    #[serde(default)]
    pub package_manager: PackageManager,

    /// Dotfiles from the project's dotfiles/ directory, symlinked into the
    /// invoking user's home (host-side, on build/update or `dcli-bootc dotfiles`)
    #[serde(default)]
    pub dotfiles: Vec<DotfileEntry>,

    /// Module that holds the RakuOS `rum` usr-overlay manifest (e.g. "overlay").
    /// When set, `dcli-bootc overlay import` snapshots the host overlay into
    /// that module, and build/update/`overlay sync` reconcile the live overlay
    /// to match it with `rum install`/`rum remove`. The module is overlay-only
    /// and should not be in `enabled_modules` (it is not baked into the image).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay_module: Option<String>,

    /// Which usr-overlay implementation to reconcile against (`auto`, `rum`,
    /// or `kinpkg`). `auto` picks RakuOS `rum` when it is present on the host,
    /// otherwise the kineticos `kinpkg` overlay.
    #[serde(default)]
    pub overlay_backend: OverlayBackend,

    /// Bake this project's own source tree into the image at
    /// `/usr/share/<name>/project/`, so a running system carries its recipe and
    /// `dcli-bootc init` can reproduce or fork it offline.
    #[serde(default, skip_serializing_if = "is_false")]
    pub include_project: bool,

    /// Kernel management: which kernel the image ships and how it is pinned.
    /// Overridable wholesale per host (see `HostConfig::kernel`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub kernel: Option<KernelConfig>,
}

impl Profile {
    /// Resolve the local tag for a build/update. An explicit `--tag` wins, then
    /// an active host overlay, then the variant default: `nvidia` for NVIDIA
    /// builds, so they cannot overwrite the default AMD/Intel image.
    pub fn effective_tag(&self, tag: Option<String>, host: Option<&str>, nvidia: bool) -> String {
        tag.or_else(|| host.map(str::to_string)).unwrap_or_else(|| {
            if nvidia {
                "nvidia".to_string()
            } else {
                self.default_tag.clone()
            }
        })
    }
}

/// A single dotfiles entry.
///
/// Plain form (`fish/`) names a file or directory under the project's
/// dotfiles/ directory and targets `~/.config/<name>`. The single-key map
/// form (`bashrc: ~/.bashrc`) sets an explicit full target path for files
/// that do not live in ~/.config.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum DotfileEntry {
    /// Plain name (e.g. `fish/`) → ~/.config/fish
    Simple(String),
    /// Single-key map of source name → full target path (e.g. `bashrc: ~/.bashrc`)
    Mapped(std::collections::HashMap<String, String>),
}

impl DotfileEntry {
    /// Name of the file/dir under the project's dotfiles/ directory
    /// (trailing slashes stripped).
    pub fn source_name(&self) -> String {
        match self {
            DotfileEntry::Simple(name) => name.trim_end_matches('/').to_string(),
            DotfileEntry::Mapped(map) => map
                .keys()
                .next()
                .map(|k| k.trim_end_matches('/').to_string())
                .unwrap_or_default(),
        }
    }

    /// Target path as configured. Plain entries default to
    /// `~/.config/<name>`; mapped entries use their explicit path. A leading
    /// `~/` is expanded against the invoking user's home at apply time.
    pub fn target(&self) -> String {
        match self {
            DotfileEntry::Simple(name) => {
                format!("~/.config/{}", name.trim_end_matches('/'))
            }
            DotfileEntry::Mapped(map) => map.values().next().cloned().unwrap_or_default(),
        }
    }
}

fn default_base_image() -> String {
    "ghcr.io/ublue-os/bazzite:stable".to_string()
}

fn is_false(value: &bool) -> bool {
    !*value
}

fn default_tag() -> String {
    "latest".to_string()
}

/// Per-host overlay loaded from hosts/<name>.yaml
///
/// A host adds modules on top of the profile's `enabled_modules`, can
/// exclude profile modules, and may override the base images.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HostConfig {
    #[serde(default)]
    pub description: Option<String>,

    /// Modules to add on top of the profile's enabled_modules
    #[serde(default)]
    pub modules: Vec<String>,

    /// Modules to remove from the profile's enabled_modules
    #[serde(default)]
    pub exclude_modules: Vec<String>,

    /// Dotfiles to add on top of the profile's dotfiles
    #[serde(default)]
    pub dotfiles: Vec<DotfileEntry>,

    /// Dotfiles to remove from the profile's dotfiles (matched by source name)
    #[serde(default)]
    pub exclude_dotfiles: Vec<String>,

    /// Optional base image override for this host
    #[serde(default)]
    pub base_image: Option<String>,

    /// Optional NVIDIA base image override for this host
    #[serde(default)]
    pub nvidia_base_image: Option<String>,

    /// Optional package manager override for this host
    #[serde(default)]
    pub package_manager: Option<PackageManager>,

    /// Optional RakuOS `rum` usr-overlay manifest module override for this
    /// host. Lets each machine reconcile its live overlay against its own
    /// module instead of the profile's shared `overlay_module`.
    #[serde(default)]
    pub overlay_module: Option<String>,

    /// Optional usr-overlay backend override for this host (`auto`, `rum`,
    /// `kinpkg`). When present it replaces the profile's `overlay_backend`.
    #[serde(default)]
    pub overlay_backend: Option<OverlayBackend>,

    /// Optional kernel override for this host. When present it replaces the
    /// profile's `kernel:` block entirely.
    #[serde(default)]
    pub kernel: Option<KernelConfig>,
}

/// A single module definition loaded from modules/*.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub description: String,

    #[serde(default)]
    pub packages: Vec<String>,

    /// Package groups to install (dnf groups, e.g. "standard", "development")
    #[serde(default)]
    pub groups: Vec<String>,

    #[serde(default)]
    pub remove: Vec<String>,

    #[serde(default)]
    pub repos: RepoConfig,

    #[serde(default)]
    pub remote_rpms: Vec<RemoteRpm>,

    /// Filenames of RPMs shipped in the project's rpms/ directory
    #[serde(default)]
    pub local_rpms: Vec<String>,

    /// Filenames of AppImages shipped in the project's appimages/ directory
    #[serde(default)]
    pub appimages: Vec<String>,

    /// Flatpak remotes, applied with `flatpak remote-add --user` on the live
    /// system (deduped by name; flathub is auto-added when none are declared).
    /// Alias of `user_flatpak_remotes`.
    #[serde(default)]
    pub flatpak_remotes: Vec<FlatpakRemote>,

    /// Flatpak apps to install into the invoking user's flatpak installation.
    /// Alias of `user_flatpaks`; flatpaks are never baked into the image.
    #[serde(default)]
    pub flatpaks: Vec<String>,

    /// Flatpak apps to remove from the invoking user's flatpak installation.
    /// Alias of `user_remove_flatpaks`.
    #[serde(default)]
    pub remove_flatpaks: Vec<String>,

    /// Flatpak remotes for user-scoped apps, added with
    /// `flatpak remote-add --user` on the live system
    #[serde(default)]
    pub user_flatpak_remotes: Vec<FlatpakRemote>,

    /// Flatpak apps to install into the invoking user's flatpak installation
    /// on the live system (not baked into the image)
    #[serde(default)]
    pub user_flatpaks: Vec<String>,

    /// Flatpak apps to remove from the invoking user's flatpak installation
    #[serde(default)]
    pub user_remove_flatpaks: Vec<String>,

    /// Directories under files/ to rsync into the image
    #[serde(default)]
    pub files: Vec<String>,

    /// Systemd services to enable (always on)
    #[serde(default)]
    pub services: Vec<String>,

    /// Systemd services with ConditionPathExists guard (first-boot only)
    #[serde(default)]
    pub first_boot_services: Vec<FirstBootService>,

    /// Inline shell script blocks injected at specific points
    #[serde(default)]
    pub scripts: Scripts,

    /// Module ordering: must come after these modules
    #[serde(default)]
    pub depends: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FirstBootService {
    pub name: String,

    /// The condition path that gates the service
    #[serde(default = "default_condition_path")]
    pub condition_path: String,

    /// The full unit file content (optional — generator can use template)
    #[serde(default)]
    pub unit_content: Option<String>,
}

fn default_condition_path() -> String {
    "/etc/systemd/system/nix-daemon.service".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct RepoConfig {
    #[serde(default)]
    pub copr: Vec<String>,

    #[serde(default)]
    pub rpm_urls: Vec<RpmRepo>,

    /// APT repository lines (e.g. "deb http://... stable main")
    #[serde(default)]
    pub apt_lines: Vec<String>,

    /// Names of already-installed .repo files to enable (e.g. "terra")
    #[serde(default)]
    pub enable: Vec<String>,

    /// GPG key URLs to import before enabling repos
    #[serde(default)]
    pub import_keys: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RpmRepo {
    pub name: String,
    pub baseurl: String,
    pub gpgkey: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FlatpakRemote {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RemoteRpm {
    pub url: String,
    pub sha256: Option<String>,
    /// Target filename under /tmp (inferred from URL if not set)
    #[serde(default)]
    pub filename: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Scripts {
    /// Runs before package installs (good for repo setup)
    #[serde(default)]
    pub pre: Option<String>,

    /// Runs after package installs, files, and services
    #[serde(default)]
    pub post: Option<String>,
}

/// Fedora stock kernel packages removed by default when a replacement kernel
/// is installed and `kernel.remove` is omitted. Removal only touches packages
/// that are actually present, so this is safe across base images.
pub const DEFAULT_STOCK_KERNEL_PACKAGES: &[&str] = &[
    "kernel",
    "kernel-core",
    "kernel-modules",
    "kernel-modules-core",
    "kernel-modules-extra",
];

/// Kernel management declared in `profile.yaml` (overridable per host).
///
/// dcli-bootc treats the kernel as a first-class concern: it adds the repo,
/// swaps the stock kernel for the declared one, optionally pins the version,
/// and regenerates module deps + initramfs. bootc supports exactly one kernel
/// per image (the `kernel` lint in `bootc container lint` is fatal), so the
/// stock kernel must be removed whenever a replacement is installed.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KernelConfig {
    /// Repos providing the kernel (same shape as module repos)
    #[serde(default)]
    pub repo: RepoConfig,

    /// Stock kernel packages to remove. Omitted (`None`) → the Fedora stock
    /// set is removed when `packages` is set; `remove: []` → remove nothing.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub remove: Option<Vec<String>>,

    /// Replacement kernel package(s) to install
    #[serde(default)]
    pub packages: Vec<String>,

    /// Optional version applied to every entry in `packages` (e.g. "6.11.5"),
    /// producing an install spec such as `kernel-cachyos-6.11.5`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,

    /// Emit `dnf versionlock` for the kernel packages after install
    #[serde(default)]
    pub versionlock: bool,

    /// Regenerate `modules.dep` and the initramfs after the swap (default
    /// true). Required for the replacement kernel to boot on a bootc system.
    #[serde(default = "default_true")]
    pub regen_initramfs: bool,
}

impl KernelConfig {
    /// Kernel packages to remove: the explicit list when set, otherwise the
    /// Fedora stock set when a replacement kernel is being installed.
    pub fn effective_remove(&self) -> Vec<String> {
        match &self.remove {
            Some(list) => list.clone(),
            None if !self.packages.is_empty() => DEFAULT_STOCK_KERNEL_PACKAGES
                .iter()
                .map(|s| s.to_string())
                .collect(),
            None => Vec::new(),
        }
    }

    /// Install specs: package names with the pinned `version` suffix applied.
    pub fn install_specs(&self) -> Vec<String> {
        match &self.version {
            Some(v) if !v.is_empty() => self
                .packages
                .iter()
                .map(|p| format!("{}-{}", p, v))
                .collect(),
            _ => self.packages.clone(),
        }
    }
}

fn default_true() -> bool {
    true
}

/// Find the profile file in a project directory (.yaml or .yml)
pub fn profile_path(project_dir: &Path) -> Option<PathBuf> {
    for name in ["profile.yaml", "profile.yml"] {
        let path = project_dir.join(name);
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Helper to load a profile from a project directory
pub fn load_profile(project_dir: &Path) -> Result<Profile> {
    let path = profile_path(project_dir).with_context(|| {
        format!(
            "No profile.yaml or profile.yml found in {}",
            project_dir.display()
        )
    })?;
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read profile: {}", path.display()))?;
    let profile: Profile = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse profile: {}", path.display()))?;
    Ok(profile)
}

/// Find a module file in the project's modules/ directory (.yaml or .yml)
pub fn module_path(project_dir: &Path, module_name: &str) -> Option<PathBuf> {
    for ext in ["yaml", "yml"] {
        let path = project_dir
            .join("modules")
            .join(format!("{}.{}", module_name, ext));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Load a specific module from the project's modules/ directory
pub fn load_module(project_dir: &Path, module_name: &str) -> Result<Module> {
    let path = module_path(project_dir, module_name)
        .with_context(|| format!("Module '{}' not found in modules/", module_name))?;
    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read module: {}", path.display()))?;
    let module: Module = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse module: {}", path.display()))?;
    Ok(module)
}

/// List all available modules in the project
pub fn list_modules(project_dir: &Path) -> Result<Vec<String>> {
    let modules_dir = project_dir.join("modules");
    if !modules_dir.exists() {
        return Ok(Vec::new());
    }

    let mut modules = Vec::new();
    for entry in std::fs::read_dir(&modules_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "yaml" || ext == "yml" {
                    if let Some(stem) = path.file_stem() {
                        modules.push(stem.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    modules.sort();
    modules.dedup();
    Ok(modules)
}

/// Find a host file in the project's hosts/ directory (.yaml or .yml)
pub fn host_path(project_dir: &Path, host_name: &str) -> Option<PathBuf> {
    for ext in ["yaml", "yml"] {
        let path = project_dir
            .join("hosts")
            .join(format!("{}.{}", host_name, ext));
        if path.exists() {
            return Some(path);
        }
    }
    None
}

/// Load a specific host overlay from the project's hosts/ directory
pub fn load_host(project_dir: &Path, host_name: &str) -> Result<HostConfig> {
    let Some(path) = host_path(project_dir, host_name) else {
        let available = list_hosts(project_dir).unwrap_or_default();
        if available.is_empty() {
            anyhow::bail!("Host '{}' not found (no host files in hosts/)", host_name);
        }
        anyhow::bail!(
            "Host '{}' not found in hosts/. Available hosts: {}",
            host_name,
            available.join(", ")
        );
    };

    let content = std::fs::read_to_string(&path)
        .with_context(|| format!("Failed to read host: {}", path.display()))?;
    let host: HostConfig = serde_yaml::from_str(&content)
        .with_context(|| format!("Failed to parse host: {}", path.display()))?;
    Ok(host)
}

/// List all available hosts in the project
pub fn list_hosts(project_dir: &Path) -> Result<Vec<String>> {
    let hosts_dir = project_dir.join("hosts");
    if !hosts_dir.exists() {
        return Ok(Vec::new());
    }

    let mut hosts = Vec::new();
    for entry in std::fs::read_dir(&hosts_dir)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension() {
                if ext == "yaml" || ext == "yml" {
                    if let Some(stem) = path.file_stem() {
                        hosts.push(stem.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    hosts.sort();
    hosts.dedup();
    Ok(hosts)
}

/// Detect this machine's hostname (Linux, no external deps)
pub fn detect_hostname() -> Option<String> {
    std::fs::read_to_string("/proc/sys/kernel/hostname")
        .ok()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
}

/// Resolve the effective profile for a host.
///
/// Host selection: explicit `host` argument wins; otherwise the machine
/// hostname is auto-detected and used if a matching hosts/<hostname>.yaml
/// exists. With no host, the plain profile is returned unchanged.
///
/// The host overlay adds `modules` on top of the profile's enabled_modules
/// (deduped, order preserved), removes `exclude_modules`, merges `dotfiles`
/// the same way (minus `exclude_dotfiles`, matched by source name), and
/// applies any base image overrides.
pub fn resolve_effective_profile(
    project_dir: &Path,
    host: Option<&str>,
) -> Result<(Profile, Option<String>)> {
    let mut profile = load_profile(project_dir)?;

    let host_name = match host {
        Some(h) => Some(h.to_string()),
        None => detect_hostname().filter(|h| host_path(project_dir, h).is_some()),
    };

    let Some(host_name) = host_name else {
        return Ok((profile, None));
    };

    let host_cfg = load_host(project_dir, &host_name)?;

    for ex in &host_cfg.exclude_modules {
        if !profile.enabled_modules.contains(ex) {
            eprintln!(
                "Warning: host '{}' excludes module '{}' which is not in the profile's enabled_modules",
                host_name, ex
            );
        }
    }

    // Merge: profile modules + host modules (dedup, order preserved) - excludes
    let mut merged: Vec<String> = Vec::new();
    for m in profile.enabled_modules.iter().chain(host_cfg.modules.iter()) {
        if !host_cfg.exclude_modules.contains(m) && !merged.contains(m) {
            merged.push(m.clone());
        }
    }
    profile.enabled_modules = merged;

    // Merge dotfiles the same way: profile + host additions (deduped by
    // source name, order preserved) minus excludes (matched by source name).
    for ex in &host_cfg.exclude_dotfiles {
        let ex = ex.trim_end_matches('/');
        if !profile.dotfiles.iter().any(|d| d.source_name() == ex) {
            eprintln!(
                "Warning: host '{}' excludes dotfiles entry '{}' which is not in the profile's dotfiles",
                host_name, ex
            );
        }
    }
    let excludes: Vec<String> = host_cfg
        .exclude_dotfiles
        .iter()
        .map(|e| e.trim_end_matches('/').to_string())
        .collect();
    let mut merged_dotfiles: Vec<DotfileEntry> = Vec::new();
    for d in profile.dotfiles.iter().chain(host_cfg.dotfiles.iter()) {
        let name = d.source_name();
        if !excludes.contains(&name) && !merged_dotfiles.iter().any(|m| m.source_name() == name) {
            merged_dotfiles.push(d.clone());
        }
    }
    profile.dotfiles = merged_dotfiles;

    if let Some(base) = host_cfg.base_image {
        profile.base_image = base;
    }
    if let Some(nv) = host_cfg.nvidia_base_image {
        profile.nvidia_base_image = Some(nv);
    }
    if let Some(pm) = host_cfg.package_manager {
        profile.package_manager = pm;
    }
    if let Some(om) = host_cfg.overlay_module {
        profile.overlay_module = Some(om);
    }
    if let Some(ob) = host_cfg.overlay_backend {
        profile.overlay_backend = ob;
    }
    // Host kernel config replaces the profile's wholesale (like base_image).
    if let Some(kernel) = host_cfg.kernel {
        profile.kernel = Some(kernel);
    }

    Ok((profile, Some(host_name)))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn kernel(packages: &[&str], remove: Option<&[&str]>, version: Option<&str>) -> KernelConfig {
        KernelConfig {
            repo: RepoConfig::default(),
            remove: remove.map(|r| r.iter().map(|s| s.to_string()).collect()),
            packages: packages.iter().map(|s| s.to_string()).collect(),
            version: version.map(|s| s.to_string()),
            versionlock: false,
            regen_initramfs: true,
        }
    }

    #[test]
    fn auto_removes_stock_set_when_replacement_declared() {
        let k = kernel(&["kernel-cachyos"], None, None);
        assert_eq!(
            k.effective_remove(),
            DEFAULT_STOCK_KERNEL_PACKAGES
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
        );
    }

    #[test]
    fn omitted_remove_is_noop_without_a_replacement() {
        assert!(kernel(&[], None, None).effective_remove().is_empty());
    }

    #[test]
    fn explicit_empty_remove_disables_the_auto_default() {
        assert!(kernel(&["kernel-cachyos"], Some(&[]), None)
            .effective_remove()
            .is_empty());
    }

    #[test]
    fn version_suffixes_install_specs() {
        let k = kernel(
            &["kernel-cachyos", "kernel-cachyos-modules"],
            Some(&[]),
            Some("6.11.5"),
        );
        assert_eq!(
            k.install_specs(),
            vec![
                "kernel-cachyos-6.11.5".to_string(),
                "kernel-cachyos-modules-6.11.5".to_string()
            ]
        );
    }
}

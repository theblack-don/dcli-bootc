use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

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
}

fn default_base_image() -> String {
    "ghcr.io/ublue-os/bazzite:stable".to_string()
}

fn default_tag() -> String {
    "latest".to_string()
}

/// A single module definition loaded from modules/*.yaml
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Module {
    pub description: String,

    #[serde(default)]
    pub packages: Vec<String>,

    #[serde(default)]
    pub remove: Vec<String>,

    #[serde(default)]
    pub repos: RepoConfig,

    #[serde(default)]
    pub remote_rpms: Vec<RemoteRpm>,

    /// Filenames of RPMs shipped in the project's rpms/ directory
    #[serde(default)]
    pub local_rpms: Vec<String>,

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

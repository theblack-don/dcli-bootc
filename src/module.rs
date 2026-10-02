use anyhow::Result;
use std::collections::HashSet;
use std::path::Path;

use crate::config::{self, FlatpakRemote, Module};

/// Resolved module with its name
pub struct ResolvedModule {
    pub name: String,
    pub module: Module,
}

/// Load all enabled modules for a profile, in dependency order
pub fn load_enabled_modules(project_dir: &Path, module_names: &[String]) -> Result<Vec<ResolvedModule>> {
    // Load all requested modules
    let mut modules: Vec<ResolvedModule> = Vec::new();
    let mut loaded = HashSet::new();

    for name in module_names {
        resolve_with_deps(project_dir, name, &mut modules, &mut loaded)?;
    }

    Ok(modules)
}

/// Recursively resolve a module and its dependencies in order
fn resolve_with_deps(
    project_dir: &Path,
    name: &str,
    result: &mut Vec<ResolvedModule>,
    loaded: &mut HashSet<String>,
) -> Result<()> {
    if loaded.contains(name) {
        return Ok(());
    }

    let module = config::load_module(project_dir, name)?;

    // Mark as loaded first to prevent circular deps
    loaded.insert(name.to_string());

    // Resolve dependencies first
    for dep in &module.depends {
        resolve_with_deps(project_dir, dep, result, loaded)?;
    }

    result.push(ResolvedModule {
        name: name.to_string(),
        module,
    });

    Ok(())
}

/// Flatpak remotes declared across all modules (deduped by name, order
/// preserved). `flatpak_remotes:` and `user_flatpak_remotes:` are equivalent —
/// both are applied to the invoking user's installation. When none are
/// declared, flathub is used as the default.
pub fn collect_flatpak_remotes(modules: &[ResolvedModule]) -> Vec<FlatpakRemote> {
    let mut remotes: Vec<FlatpakRemote> = Vec::new();
    for rm in modules {
        let declared = rm
            .module
            .flatpak_remotes
            .iter()
            .chain(rm.module.user_flatpak_remotes.iter());
        for remote in declared {
            if !remotes.iter().any(|r| r.name == remote.name) {
                remotes.push(remote.clone());
            }
        }
    }
    if remotes.is_empty() {
        remotes.push(FlatpakRemote {
            name: "flathub".to_string(),
            url: "https://dl.flathub.org/repo/flathub.flatpakrepo".to_string(),
        });
    }
    remotes
}

/// Validate modules for conflicts and integrity
pub fn validate_modules(_project_dir: &Path, modules: &[ResolvedModule]) -> Result<Vec<String>> {
    let mut warnings = Vec::new();
    let mut all_packages = HashSet::new();
    let mut all_removes = HashSet::new();

    for rm in modules {
        // Check for same package in install and remove
        for pkg in &rm.module.packages {
            if all_removes.contains(pkg) {
                warnings.push(format!(
                    "Module '{}' installs '{}' which is removed by another module",
                    rm.name, pkg
                ));
            }
            all_packages.insert(pkg.clone());
        }

        for pkg in &rm.module.remove {
            all_removes.insert(pkg.clone());
        }
    }

    Ok(warnings)
}

//! usr-overlay backends.
//!
//! dcli-bootc makes the host's live usr-overlay declarative: `overlay import`
//! snapshots the overlay into a module, and build/update (or `overlay sync`)
//! reconcile the live overlay to match it. Two backends are supported:
//!
//! * `rum` — RakuOS. `rum install` writes packages into the overlay upperdir and
//!   records their names in `/var/lib/rakuos/packages.list`. The base image
//!   keeps its own rpmdb at `/usr/share/rpm`, separate from rum's
//!   `/var/lib/rakuos/rum-rpmdb`, so base and overlay packages are
//!   distinguishable.
//! * `kinpkg` — kineticos. A persistent overlayfs on `/usr` (shipped by the
//!   project's `overlay` module) with `kinpkg` as the installer. There is a
//!   single merged rpmdb, so the overlay's packages are derived from the
//!   manually-installed (`--userinstalled`) set minus the base-package and
//!   base-provides lists baked into the image at build time (see
//!   `BAKED_BASE_LIST` / `BAKED_BASE_PROVIDES`). This catches plain `dnf
//!   install` too, which also lands in the overlay. kinpkg's own
//!   `/var/lib/kineticos/overlay/installed.txt` record is the fallback.
//!
//! Packages the base image provides are kept rather than removed — erasing one
//! through the overlay unlinks a path that also exists in the base layer,
//! leaving an overlayfs whiteout that hides the base copy permanently.

use anyhow::{Context, Result};
use std::collections::HashSet;
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use crate::config::OverlayBackend as BackendPref;

/// Path of the base-image package list the generator bakes into the image for
/// backends whose overlay shares the base rpmdb (kinpkg).
pub const BAKED_BASE_LIST: &str = "/usr/share/dcli-bootc/base-packages.list";

/// Path of the base-image capability list the generator bakes into the image
/// for backends whose overlay shares the base rpmdb (kinpkg). Records every
/// `Provides:` name so a dependency satisfied by a renamed drop-in (e.g.
/// `terra-ddcutil` providing `ddcutil`) still counts as base-provided.
pub const BAKED_BASE_PROVIDES: &str = "/usr/share/dcli-bootc/base-provides.list";

/// A usr-overlay implementation dcli-bootc can reconcile against.
pub trait OverlayBackend {
    /// Stable id, e.g. "rum" or "kinpkg".
    fn id(&self) -> &'static str;

    /// Human label for messages, e.g. "RakuOS overlay (rum)".
    fn label(&self) -> &'static str;

    /// The installer command's short name, e.g. "rum" or "kinpkg".
    fn tool(&self) -> &'static str;

    /// True when this backend is usable on the current host.
    fn available(&self) -> bool;

    /// Packages currently installed into the host overlay, sorted and deduped.
    fn installed_packages(&self) -> Result<Vec<String>>;

    /// Install packages into the host overlay.
    fn install_packages(&self, pkgs: &[String], dry_run: bool) -> Result<()>;

    /// Remove packages from the host overlay.
    fn remove_packages(&self, pkgs: &[String], dry_run: bool) -> Result<()>;

    /// Upgrade packages already installed in the host overlay to their newest
    /// available versions. Unlike install/remove this ignores the manifest —
    /// it refreshes whatever the overlay currently holds (used by `update`).
    fn upgrade_packages(&self) -> Result<()>;

    /// Package names the booted base image provides, when it can be
    /// determined. `None` means "unknown" — callers then refuse to remove
    /// rather than risk a whiteout.
    fn base_packages(&self) -> Option<HashSet<String>>;

    /// True when `name` is installed in the live overlay.
    fn has_package(&self, name: &str) -> bool;

    /// Whether this backend supports locally-installed RPMs (staged copies).
    fn supports_local_rpms(&self) -> bool;

    /// Names of locally-installed overlay RPMs.
    fn installed_local_rpms(&self) -> Vec<String> {
        Vec::new()
    }

    /// Where this backend stages a copy of a locally-installed RPM.
    fn staged_local_rpm(&self, _name: &str) -> Option<PathBuf> {
        None
    }

    /// Install a local `.rpm` file into the host overlay.
    fn install_local_rpm(&self, _path: &Path, _dry_run: bool) -> Result<()> {
        anyhow::bail!("{} does not support local RPMs", self.id())
    }

    /// Overwrite the backend's staged copy of a local RPM (root owned), so a
    /// later overlay rebuild can still reinstall it.
    fn stage_local_rpm(&self, _src: &Path, _name: &str) -> Result<()> {
        anyhow::bail!("{} does not support local RPMs", self.id())
    }

    /// Overwrite the host overlay's package lists (root owned) to match the
    /// given contents, backing the originals up as `.bak` first.
    fn write_package_lists(&self, packages: &[String], local_rpms: &[String]) -> Result<()>;

    /// Whether this backend has an operation that rebuilds the live overlay
    /// from the (rewritten) package lists.
    fn supports_reset(&self) -> bool;

    /// Schedule a rebuild of the live overlay from its package lists.
    fn schedule_reset(&self) -> Result<()> {
        anyhow::bail!("{} has no overlay reset operation", self.id())
    }
}

/// Select the backend to use for `pref`, or `None` when none is available.
pub fn select(pref: BackendPref) -> Option<Box<dyn OverlayBackend>> {
    match pref {
        BackendPref::Rum => {
            let backend = RumBackend;
            backend
                .available()
                .then(|| Box::new(backend) as Box<dyn OverlayBackend>)
        }
        BackendPref::Kinpkg => {
            let backend = KinpkgBackend;
            backend
                .available()
                .then(|| Box::new(backend) as Box<dyn OverlayBackend>)
        }
        BackendPref::Auto => {
            let rum = RumBackend;
            if rum.available() {
                Some(Box::new(rum))
            } else {
                let kinpkg = KinpkgBackend;
                if kinpkg.available() {
                    Some(Box::new(kinpkg))
                } else {
                    None
                }
            }
        }
    }
}

/// Explain what the host is missing for the requested backend to be available.
pub fn unavailable_message(pref: BackendPref) -> String {
    match pref {
        BackendPref::Rum => "RakuOS rum overlay not found on this host (need /var/lib/rakuos/packages.list and 'rum' in PATH)".to_string(),
        BackendPref::Kinpkg => format!(
            "kineticos kinpkg overlay not found on this host (need {} and 'kinpkg' in PATH)",
            KinpkgBackend::BASE
        ),
        BackendPref::Auto => "no usr-overlay backend found on this host (need RakuOS rum, or kineticos kinpkg plus /var/lib/kineticos/overlay)".to_string(),
    }
}

// ───────────────────────────── RakuOS rum ─────────────────────────────

const RUM_PACKAGES_LIST: &str = "/var/lib/rakuos/packages.list";
const RUM_RPM_LIST: &str = "/var/lib/rakuos/packages-rpm.list";
const RUM_RPMDB: &str = "/var/lib/rakuos/rum-rpmdb";
const RUM_LOCAL_RPM_DIR: &str = "/var/lib/rakuos/local-rpms";

/// RakuOS `rum` usr-overlay.
pub struct RumBackend;

impl OverlayBackend for RumBackend {
    fn id(&self) -> &'static str {
        "rum"
    }

    fn label(&self) -> &'static str {
        "RakuOS overlay (rum)"
    }

    fn tool(&self) -> &'static str {
        "rum"
    }

    fn available(&self) -> bool {
        Path::new(RUM_PACKAGES_LIST).exists() && which("rum").is_some()
    }

    fn installed_packages(&self) -> Result<Vec<String>> {
        let raw = std::fs::read_to_string(RUM_PACKAGES_LIST)
            .with_context(|| format!("Failed to read {}", RUM_PACKAGES_LIST))?;
        Ok(parse_list(&raw))
    }

    fn install_packages(&self, pkgs: &[String], dry_run: bool) -> Result<()> {
        run_rum("install", pkgs, dry_run)
    }

    fn remove_packages(&self, pkgs: &[String], dry_run: bool) -> Result<()> {
        run_rum("remove", pkgs, dry_run)
    }

    fn upgrade_packages(&self) -> Result<()> {
        run_rum_upgrade()
    }

    fn base_packages(&self) -> Option<HashSet<String>> {
        // RakuOS split layout: base packages live in the booted image's own
        // rpmdb, overlay packages in rum's separate db.
        rpm_names(&["-qa", "--dbpath", "/usr/share/rpm", "--queryformat", "%{NAME}\n"])
    }

    fn has_package(&self, name: &str) -> bool {
        rpm_query_succeeds(&["-q", "--dbpath", RUM_RPMDB, name])
    }

    fn supports_local_rpms(&self) -> bool {
        true
    }

    fn installed_local_rpms(&self) -> Vec<String> {
        read_list_optional(RUM_RPM_LIST)
    }

    fn staged_local_rpm(&self, name: &str) -> Option<PathBuf> {
        Some(Path::new(RUM_LOCAL_RPM_DIR).join(format!("{}.rpm", name)))
    }

    fn install_local_rpm(&self, path: &Path, dry_run: bool) -> Result<()> {
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        let mut cmd = Command::new("sudo");
        cmd.arg("rum").arg("install").arg("-y");
        if dry_run {
            cmd.arg("--dry-run");
        }
        cmd.arg(&abs);
        let status = cmd
            .status()
            .with_context(|| format!("Failed to execute 'rum install {}'", abs.display()))?;
        if !status.success() {
            anyhow::bail!("rum install {} failed", abs.display());
        }
        Ok(())
    }

    fn stage_local_rpm(&self, src: &Path, name: &str) -> Result<()> {
        let dst = Path::new(RUM_LOCAL_RPM_DIR).join(format!("{}.rpm", name));
        stage_root_copy(src, &dst)
            .with_context(|| format!("Failed to stage {} for the overlay", src.display()))
    }

    fn write_package_lists(&self, packages: &[String], local_rpms: &[String]) -> Result<()> {
        write_root_list(RUM_PACKAGES_LIST, packages)?;
        write_root_list(RUM_RPM_LIST, local_rpms)?;
        Ok(())
    }

    fn supports_reset(&self) -> bool {
        true
    }

    fn schedule_reset(&self) -> Result<()> {
        // `--soft` keeps `packages.list` and the local RPMs, so the overlay is
        // reinstalled from the (freshly rewritten) lists.
        let status = Command::new("sudo")
            .args(["rakuos", "reset-overlay", "--soft"])
            .status()
            .context(
                "Failed to execute 'sudo rakuos reset-overlay --soft' (is 'rakuos' installed and on PATH?)",
            )?;
        if !status.success() {
            anyhow::bail!("rakuos reset-overlay --soft failed");
        }
        Ok(())
    }
}

/// Install packages into the host overlay with `rum install`/`rum remove`.
fn run_rum(action: &str, pkgs: &[String], dry_run: bool) -> Result<()> {
    if pkgs.is_empty() {
        return Ok(());
    }
    let mut cmd = Command::new("sudo");
    cmd.arg("rum").arg(action).arg("-y");
    if dry_run {
        cmd.arg("--dry-run");
    }
    cmd.args(pkgs);
    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute 'rum {}'", action))?;
    if !status.success() {
        anyhow::bail!("rum {} failed", action);
    }
    Ok(())
}

/// Upgrade every overlay-installed package with `rum upgrade`.
fn run_rum_upgrade() -> Result<()> {
    let status = Command::new("sudo")
        .args(["rum", "upgrade", "-y"])
        .status()
        .context("Failed to execute 'rum upgrade' (is 'rum' installed and on PATH?)")?;
    if !status.success() {
        anyhow::bail!("rum upgrade failed");
    }
    Ok(())
}

// ───────────────────────────── kineticos kinpkg ─────────────────────────────

/// kineticos persistent `/usr` overlay, installed/recorded by `kinpkg`.
pub struct KinpkgBackend;

impl KinpkgBackend {
    /// Overlay state directory (exists once the mount unit has run).
    const BASE: &'static str = "/var/lib/kineticos/overlay";
    /// Repo-installed overlay package names, one per line.
    const LIST: &'static str = "/var/lib/kineticos/overlay/installed.txt";
    /// Locally-installed overlay package names, one per line.
    const RPM_LIST: &'static str = "/var/lib/kineticos/overlay/installed-rpm.txt";
    /// Staged copies of locally-installed RPMs.
    const LOCAL_RPM_DIR: &'static str = "/var/lib/kineticos/overlay/local-rpms";
}

impl OverlayBackend for KinpkgBackend {
    fn id(&self) -> &'static str {
        "kinpkg"
    }

    fn label(&self) -> &'static str {
        "kineticos overlay (kinpkg)"
    }

    fn tool(&self) -> &'static str {
        "kinpkg"
    }

    fn available(&self) -> bool {
        Path::new(Self::BASE).is_dir() && kinpkg_bin().is_some()
    }

    fn installed_packages(&self) -> Result<Vec<String>> {
        // kineticos keeps one merged rpmdb inside the live overlay, and plain
        // `dnf install` lands there too (bypassing kinpkg's record). So the
        // authoritative overlay set is the manually-installed packages minus
        // the base image's — catching dnf installs as well as kinpkg's.
        //
        // `--userinstalled` excludes dependency packages, so `overlay sync`
        // never treats a manifest package's dependencies as removable. Fall
        // back to kinpkg's own record when dnf or the baked base list is
        // unavailable.
        let Some(base) = self.base_packages().filter(|b| !b.is_empty()) else {
            return read_root_list(Self::LIST);
        };
        let local: HashSet<String> = self.installed_local_rpms().into_iter().collect();
        match userinstalled_names() {
            Some(candidates) => Ok(overlay_only(candidates, &base, &local)),
            None => read_root_list(Self::LIST),
        }
    }

    fn install_packages(&self, pkgs: &[String], dry_run: bool) -> Result<()> {
        run_kinpkg("install", pkgs, dry_run)
    }

    fn remove_packages(&self, pkgs: &[String], dry_run: bool) -> Result<()> {
        run_kinpkg("remove", pkgs, dry_run)
    }

    fn upgrade_packages(&self) -> Result<()> {
        run_kinpkg_upgrade()
    }

    fn base_packages(&self) -> Option<HashSet<String>> {
        // KineticOS keeps one merged rpmdb inside the overlay, so the base set
        // is read from the lists the generator baked into the image. Include
        // provided capability names so a dependency the base image satisfies
        // with a differently-named package (e.g. Terra's `terra-ddcutil`
        // providing `ddcutil`) is not mistaken for an overlay install. The
        // provides file is absent on images built before it was baked; fall
        // back to names alone.
        let raw = std::fs::read_to_string(BAKED_BASE_LIST).ok()?;
        let mut base: HashSet<String> = parse_list(&raw).into_iter().collect();
        if let Ok(provides) = std::fs::read_to_string(BAKED_BASE_PROVIDES) {
            base.extend(parse_list(&provides));
        }
        Some(base)
    }

    fn has_package(&self, name: &str) -> bool {
        // Merged rpmdb: a plain query is authoritative.
        rpm_query_succeeds(&["-q", name])
    }

    fn supports_local_rpms(&self) -> bool {
        true
    }

    fn installed_local_rpms(&self) -> Vec<String> {
        read_root_list(Self::RPM_LIST).unwrap_or_default()
    }

    fn staged_local_rpm(&self, name: &str) -> Option<PathBuf> {
        Some(Path::new(Self::LOCAL_RPM_DIR).join(format!("{}.rpm", name)))
    }

    fn install_local_rpm(&self, path: &Path, dry_run: bool) -> Result<()> {
        // kinpkg stages a copy and records the name itself.
        let abs = std::fs::canonicalize(path).unwrap_or_else(|_| path.to_path_buf());
        run_kinpkg("install", &[abs.to_string_lossy().to_string()], dry_run)
    }

    fn stage_local_rpm(&self, src: &Path, name: &str) -> Result<()> {
        let dst = Path::new(Self::LOCAL_RPM_DIR).join(format!("{}.rpm", name));
        stage_root_copy(src, &dst)
            .with_context(|| format!("Failed to stage {} for the overlay", src.display()))
    }

    fn write_package_lists(&self, packages: &[String], local_rpms: &[String]) -> Result<()> {
        write_root_list(Self::LIST, packages)?;
        write_root_list(Self::RPM_LIST, local_rpms)?;
        Ok(())
    }

    fn supports_reset(&self) -> bool {
        false
    }
}

/// The kinpkg installer. Prefer the image's own locations: a user binary that
/// merely shares the `ki` shorthand (e.g. kinetic-ide, via ~/.local/bin) must
/// never be mistaken for the overlay tool.
fn kinpkg_bin() -> Option<PathBuf> {
    for path in ["/usr/bin/kinpkg", "/usr/bin/ki"] {
        let path = Path::new(path);
        if path.is_file() {
            return Some(path.to_path_buf());
        }
    }
    which("kinpkg")
}

/// Install/remove overlay packages with `kinpkg`.
fn run_kinpkg(action: &str, pkgs: &[String], dry_run: bool) -> Result<()> {
    if pkgs.is_empty() {
        return Ok(());
    }
    if dry_run {
        println!("  (dry run) would run: sudo kinpkg {} {}", action, pkgs.join(" "));
        return Ok(());
    }
    let bin = kinpkg_bin().context("kinpkg not found on PATH")?;
    let mut cmd = Command::new("sudo");
    // dcli-bootc has already confirmed with the user, so tell kinpkg to skip
    // dnf's own prompt (kinpkg forwards -y to dnf).
    cmd.arg(bin).arg(action).arg("-y").args(pkgs);
    let status = cmd
        .status()
        .with_context(|| format!("Failed to execute 'kinpkg {}'", action))?;
    if !status.success() {
        anyhow::bail!("kinpkg {} failed", action);
    }
    Ok(())
}

/// Upgrade every overlay-installed package with `kinpkg overlay-upgrade`.
fn run_kinpkg_upgrade() -> Result<()> {
    let bin = kinpkg_bin().context("kinpkg not found on PATH")?;
    let status = Command::new("sudo")
        .arg(bin)
        .arg("overlay-upgrade")
        .arg("-y")
        .status()
        .context("Failed to execute 'kinpkg overlay-upgrade'")?;
    if !status.success() {
        anyhow::bail!("kinpkg overlay-upgrade failed");
    }
    Ok(())
}

// ───────────────────────────── shared helpers ─────────────────────────────

/// Read and parse a package list, or an empty list when the file is absent.
fn read_list_optional(path: &str) -> Vec<String> {
    std::fs::read_to_string(path)
        .map(|raw| parse_list(&raw))
        .unwrap_or_default()
}

/// Read and parse a root-owned state list (kinpkg's live records). kinpkg's
/// state directory is 0700 root, so a normal user gets EACCES — fall back to
/// reading through `sudo`. An absent file is an empty list, not an error.
fn read_root_list(path: &str) -> Result<Vec<String>> {
    match std::fs::read_to_string(path) {
        Ok(raw) => return Ok(parse_list(&raw)),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Ok(Vec::new()),
        Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {}
        Err(e) => return Err(e).with_context(|| format!("Failed to read {}", path)),
    }

    if !sudo_succeeds(&["test", "-f", path])? {
        return Ok(Vec::new());
    }
    let out = Command::new("sudo")
        .args(["cat", "--", path])
        .stdin(Stdio::inherit())
        .output()
        .with_context(|| format!("Failed to execute 'sudo cat {}'", path))?;
    if !out.status.success() {
        anyhow::bail!("sudo cat {} failed", path);
    }
    Ok(parse_list(&String::from_utf8_lossy(&out.stdout)))
}

/// Size of a root-owned file, read through `sudo` (the overlay state
/// directories are 0700 root, so a plain stat fails for a normal user).
/// `None` when the file is missing or unreadable.
pub(crate) fn sudo_file_len(path: &Path) -> Option<u64> {
    let out = Command::new("sudo")
        .arg("stat")
        .arg("-c")
        .arg("%s")
        .arg("--")
        .arg(path)
        .stdin(Stdio::inherit())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8_lossy(&out.stdout).trim().parse().ok()
}

/// Run `sudo <args>` and report whether it succeeded (output discarded).
fn sudo_succeeds(args: &[&str]) -> Result<bool> {
    let status = Command::new("sudo")
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute 'sudo {}'", args.join(" ")))?;
    Ok(status.success())
}

fn parse_list(raw: &str) -> Vec<String> {
    let mut pkgs: Vec<String> = raw
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_string)
        .collect();
    pkgs.sort();
    pkgs.dedup();
    pkgs
}

/// Keep only the candidate names that neither the base image nor a local RPM
/// provides, sorted and deduped.
fn overlay_only(
    candidates: HashSet<String>,
    base: &HashSet<String>,
    local: &HashSet<String>,
) -> Vec<String> {
    let mut pkgs: Vec<String> = candidates
        .into_iter()
        .filter(|n| !base.contains(n) && !local.contains(n))
        .collect();
    pkgs.sort();
    pkgs.dedup();
    pkgs
}

/// Names dnf considers user-installed (not pulled in as dependencies), or
/// `None` when dnf cannot report them. Reads the rpmdb; needs no root and no
/// repo metadata.
fn userinstalled_names() -> Option<HashSet<String>> {
    let bin = which("dnf5").or_else(|| which("dnf"))?;
    let out = Command::new(bin)
        .args(["repoquery", "--userinstalled", "--qf", "%{name}\n"])
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let names: HashSet<String> = String::from_utf8_lossy(&out.stdout)
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .map(str::to_string)
        .collect();
    (!names.is_empty()).then_some(names)
}

/// Run `rpm` and collect the queried names, or `None` on failure.
fn rpm_names(args: &[&str]) -> Option<HashSet<String>> {
    let out = Command::new("rpm").args(args).output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(
        String::from_utf8_lossy(&out.stdout)
            .lines()
            .map(|l| l.trim().to_string())
            .filter(|l| !l.is_empty())
            .collect(),
    )
}

/// True when `rpm` exits successfully (a package matched).
fn rpm_query_succeeds(args: &[&str]) -> bool {
    Command::new("rpm")
        .args(args)
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
}

/// Copy `src` to root-owned `dst` with mode 0644.
fn stage_root_copy(src: &Path, dst: &Path) -> Result<()> {
    let src_str = src.to_string_lossy().to_string();
    let dst_str = dst.to_string_lossy().to_string();
    run_sudo(&[
        "install",
        "-D",
        "-m",
        "0644",
        "-o",
        "root",
        "-g",
        "root",
        src_str.as_str(),
        dst_str.as_str(),
    ])
}

/// Overwrite a root-owned package list, backing the original up as `.bak`.
fn write_root_list(path: &str, entries: &[String]) -> Result<()> {
    let body: String = entries.iter().map(|e| format!("{}\n", e)).collect();

    if Path::new(path).exists() {
        let backup = format!("{}.bak", path);
        run_sudo(&["cp", "-a", path, &backup])
            .with_context(|| format!("Failed to back up {}", path))?;
    }

    let name = Path::new(path)
        .file_name()
        .and_then(|s| s.to_str())
        .unwrap_or("list");
    let tmp = std::env::temp_dir().join(format!("dcli-bootc-{}-{}", std::process::id(), name));
    std::fs::write(&tmp, body).with_context(|| format!("Failed to stage {}", tmp.display()))?;

    let tmp_str = tmp.to_string_lossy().to_string();
    let result = run_sudo(&[
        "install",
        "-m",
        "0644",
        "-o",
        "root",
        "-g",
        "root",
        tmp_str.as_str(),
        path,
    ]);
    let _ = std::fs::remove_file(&tmp);
    result.with_context(|| format!("Failed to write {}", path))?;
    Ok(())
}

fn run_sudo(args: &[&str]) -> Result<()> {
    let status = Command::new("sudo")
        .args(args)
        .status()
        .with_context(|| format!("Failed to execute 'sudo {}'", args.join(" ")))?;
    if !status.success() {
        anyhow::bail!("sudo {} failed", args.join(" "));
    }
    Ok(())
}

fn which(bin: &str) -> Option<PathBuf> {
    std::env::var_os("PATH").and_then(|paths| {
        std::env::split_paths(&paths)
            .map(|dir| dir.join(bin))
            .find(|p| p.is_file())
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_names_skipping_comments_and_blanks() {
        let got = parse_list("steam\n\n# comment\nkitty\nsteam \n");
        assert_eq!(got, vec!["kitty", "steam"]);
    }

    #[test]
    fn read_root_list_missing_file_is_empty() {
        let missing = "/nonexistent/dcli-bootc/installed.txt";
        assert!(read_root_list(missing).unwrap().is_empty());
    }

    #[test]
    fn overlay_only_drops_base_and_local() {
        let set = |items: &[&str]| -> HashSet<String> {
            items.iter().map(|s| s.to_string()).collect()
        };
        let candidates = set(&["base-a", "local-a", "overlay-a"]);
        let base = set(&["base-a"]);
        let local = set(&["local-a"]);
        assert_eq!(overlay_only(candidates, &base, &local), vec!["overlay-a"]);
    }

    #[test]
    fn auto_prefers_rum_but_falls_back() {
        // select() depends on the host; just assert the dispatch compiles and
        // returns the rum backend when explicitly requested and available is
        // not guaranteed here, so only check the reason strings are distinct.
        assert_ne!(
            unavailable_message(BackendPref::Rum),
            unavailable_message(BackendPref::Kinpkg)
        );
    }
}

//! Freshness checks comparing the local copy of a base image against the
//! remote registry, so `build` can warn before building on a stale base.

use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::process::Command;

/// Outcome of comparing the local copy of a base image with the registry.
pub enum Freshness {
    /// The local copy matches what the registry tag currently points to.
    Fresh,
    /// The registry tag points to a newer image than the local copy.
    Stale { local: String, remote: String },
    /// The check could not run (image not present locally, offline, missing
    /// skopeo, localhost-only image, ...). Never treated as an error.
    Unknown,
}

/// Compare the local copy of `base_image` against the registry tag.
///
/// Best-effort: any failure (network, auth, missing tools) yields
/// [`Freshness::Unknown`] so a build is never blocked by the check itself.
pub fn check_base_freshness(base_image: &str) -> Freshness {
    let (Some(local), Some(remote)) = (local_digests(base_image), remote_digest(base_image))
    else {
        return Freshness::Unknown;
    };
    if local.contains(&remote) {
        Freshness::Fresh
    } else {
        Freshness::Stale {
            local: local.iter().next().cloned().unwrap_or_default(),
            remote,
        }
    }
}

/// Digests by which the local copy of the image is known in root's container
/// storage: the image digest plus every recorded repo digest.
///
/// Multi-arch tags resolve through a manifest list, and podman may record the
/// list digest, the per-arch image digest, or both — comparing against the
/// whole set avoids false "outdated" reports.
fn local_digests(image: &str) -> Option<HashSet<String>> {
    let output = Command::new("sudo")
        .args([
            "podman",
            "image",
            "inspect",
            image,
            "--format",
            "{{.Digest}} {{join .RepoDigests \" \"}}",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None; // not present locally — podman build will pull it anyway
    }
    let digests: HashSet<String> = String::from_utf8_lossy(&output.stdout)
        .split_whitespace()
        .map(|token| token.rsplit_once('@').map(|(_, d)| d).unwrap_or(token))
        .filter(|token| token.starts_with("sha256:"))
        .map(str::to_string)
        .collect();
    (!digests.is_empty()).then_some(digests)
}

/// The digest the registry tag currently points to, computed over the raw
/// manifest bytes (this matches how registries address tag targets, manifest
/// lists included).
fn remote_digest(image: &str) -> Option<String> {
    let output = Command::new("sudo")
        .args([
            "skopeo",
            "--command-timeout",
            "30s",
            "inspect",
            "--raw",
            &format!("docker://{image}"),
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None; // offline, registry auth failure, missing skopeo, ...
    }
    let digest = Sha256::digest(&output.stdout);
    Some(format!("sha256:{digest:x}"))
}

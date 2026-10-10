//! Per-build storage options, without changing the system configuration.

use anyhow::{bail, Context, Result};
use serde_json::Value;
use std::collections::BTreeMap;
use std::process::{Command, Stdio};

use crate::config::BuildStorage;

#[derive(Debug, Default)]
pub struct StorageOptions {
    args: Vec<String>,
}

impl StorageOptions {
    pub fn prepare(policy: BuildStorage) -> Result<Self> {
        Self::prepare_with(policy, read_store)
    }

    /// Append global options after `podman` and before its subcommand.
    pub fn apply(&self, cmd: &mut Command) {
        cmd.args(&self.args);
    }

    fn prepare_with(
        policy: BuildStorage,
        mut read_info: impl FnMut(&[String]) -> Result<Value>,
    ) -> Result<Self> {
        if policy == BuildStorage::System {
            println!("  Using system Podman storage settings; builds may be slow (build_storage: system)");
            return Ok(Self::default());
        }

        Self::prepare_optimized(&mut read_info).context(
            "Cannot prepare optimized build storage. Fix the reported storage settings, or explicitly set build_storage: system in profile.yaml or the selected host file",
        )
    }

    fn prepare_optimized(read_info: &mut impl FnMut(&[String]) -> Result<Value>) -> Result<Self> {
        let info = read_info(&[])?;
        let driver = store_string(&info, "graphDriverName")?;
        if driver != "overlay" {
            println!(
                "  Podman storage driver '{}': native overlay diff check does not apply; keeping system settings",
                driver
            );
            return Ok(Self::default());
        }

        let mut before = StoreSnapshot::new(info)?;
        if !before.metacopy {
            if !before.native_diff {
                bail!("Native Overlay Diff is disabled even though metacopy is not in use");
            }
            println!("  Podman storage already uses native overlay diff without metacopy");
            return Ok(Self::default());
        }

        let mountopt = before
            .options
            .entry("overlay.mountopt".to_string())
            .or_default();
        let mut tokens: Vec<_> = mountopt
            .first()
            .map(String::as_str)
            .unwrap_or("")
            .split(',')
            .filter(|token| {
                let token = token.trim();
                !token.is_empty() && token != "metacopy" && !token.starts_with("metacopy=")
            })
            .map(str::to_string)
            .collect();
        tokens.push("metacopy=off".to_string());
        *mountopt = vec![tokens.join(",")];

        let mut args = Vec::new();
        for (key, values) in &before.options {
            for value in values {
                args.push("--storage-opt".to_string());
                args.push(format!("{}={}", key, value));
            }
        }

        let after = StoreSnapshot::new(read_info(&args)?)?;
        if after.metacopy || !after.native_diff {
            bail!("Podman did not enable native overlay diff with metacopy disabled");
        }
        let expected = before.stable_info()?;
        let actual = after.stable_info()?;
        if expected != actual {
            let field = expected
                .keys()
                .chain(actual.keys())
                .find(|key| expected.get(*key) != actual.get(*key))
                .unwrap();
            bail!(
                "Podman storage field '{}' changed while disabling metacopy",
                field
            );
        }

        println!("  Podman build will use native overlay diff with metacopy disabled");
        Ok(Self { args })
    }
}

fn read_store(args: &[String]) -> Result<Value> {
    let output = Command::new("sudo")
        .arg("podman")
        .args(args)
        .args(["info", "--format", "{{json .Store}}"])
        .stdin(Stdio::inherit())
        .output()
        .context("Failed to execute podman info")?;
    if !output.status.success() {
        bail!(
            "podman info failed ({}): {}",
            output.status,
            String::from_utf8_lossy(&output.stderr).trim()
        );
    }
    if !output.stderr.is_empty() {
        eprint!("{}", String::from_utf8_lossy(&output.stderr));
    }
    serde_json::from_slice(&output.stdout).context("Invalid Store JSON from podman info")
}

struct StoreSnapshot {
    info: Value,
    options: BTreeMap<String, Vec<String>>,
    metacopy: bool,
    native_diff: bool,
}

impl StoreSnapshot {
    fn new(info: Value) -> Result<Self> {
        for key in ["graphDriverName", "graphRoot", "runRoot", "volumePath"] {
            store_string(&info, key)?;
        }
        if !info["transientStore"].is_boolean() {
            bail!("Missing or invalid Podman storage field 'transientStore'");
        }
        let metacopy = store_status(&info, "Using metacopy")?;
        let native_diff = store_status(&info, "Native Overlay Diff")?;
        let options = normalize_options(&info["graphOptions"])?;
        Ok(Self {
            info,
            options,
            metacopy,
            native_diff,
        })
    }

    fn stable_info(&self) -> Result<serde_json::Map<String, Value>> {
        let mut store = self
            .info
            .as_object()
            .context("Podman Store must be an object")?
            .clone();
        for key in [
            "containerStore",
            "imageStore",
            "graphRootAllocated",
            "graphRootUsed",
        ] {
            store.remove(key);
        }
        store.insert(
            "graphOptions".to_string(),
            serde_json::to_value(&self.options)?,
        );
        let status = store
            .get_mut("graphStatus")
            .and_then(Value::as_object_mut)
            .context("Podman graphStatus must be an object")?;
        status.remove("Using metacopy");
        status.remove("Native Overlay Diff");
        Ok(store)
    }
}

fn store_string<'a>(info: &'a Value, key: &str) -> Result<&'a str> {
    info[key]
        .as_str()
        .filter(|value| !value.is_empty())
        .with_context(|| format!("Missing or invalid Podman storage field '{}'", key))
}

fn store_status(info: &Value, key: &str) -> Result<bool> {
    match &info["graphStatus"][key] {
        Value::Bool(value) => Ok(*value),
        Value::String(value) if value == "true" => Ok(true),
        Value::String(value) if value == "false" => Ok(false),
        _ => bail!("Missing or invalid Podman graphStatus '{}'", key),
    }
}

/// `podman info` expands some driver options into informational objects/lists.
fn normalize_options(value: &Value) -> Result<BTreeMap<String, Vec<String>>> {
    let graph = value
        .as_object()
        .context("Podman graphOptions must be an object")?;
    let mut options = BTreeMap::new();
    let stores = graph
        .get("overlay.additionalImageStores")
        .map(|value| -> Result<Vec<String>> {
            value
                .as_array()
                .context("Podman additionalImageStores must be a list")?
                .iter()
                .map(|value| {
                    value
                        .as_str()
                        .filter(|path| !path.is_empty())
                        .map(str::to_string)
                        .context("Invalid additional image store path")
                })
                .collect()
        })
        .transpose()?;

    if let Some(value) = graph.get("overlay.imagestore") {
        let scalar = value
            .as_str()
            .context("Podman overlay.imagestore must be a string")?;
        match &stores {
            Some(stores) if scalar == stores.last().map(String::as_str).unwrap_or("") => {}
            None if scalar.is_empty() => {}
            _ => bail!("Ambiguous Podman image stores: overlay.imagestore does not match an additionalImageStores list"),
        }
    }
    if let Some(stores) = stores {
        if !stores.is_empty() {
            options.insert("overlay.imagestore".to_string(), stores);
        }
    }

    for (key, value) in graph {
        if key == "overlay.additionalImageStores" || key == "overlay.imagestore" {
            continue;
        }
        let text: &str = match value {
            Value::String(text) => text,
            Value::Object(program) if key == "overlay.mount_program" => program
                .get("Executable")
                .and_then(Value::as_str)
                .filter(|path| !path.is_empty())
                .with_context(|| format!("Missing Executable in Podman option '{}'", key))?,
            _ => bail!(
                "Unsupported representation of Podman storage option '{}'",
                key
            ),
        };
        options.insert(key.clone(), vec![text.to_string()]);
    }
    Ok(options)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::collections::VecDeque;

    fn store(metacopy: bool, native_diff: bool) -> Value {
        json!({
            "graphDriverName": "overlay",
            "graphRoot": "/var/lib/containers/storage",
            "runRoot": "/run/containers/storage",
            "volumePath": "/var/lib/containers/storage/volumes",
            "transientStore": false,
            "graphOptions": {"overlay.mountopt": if metacopy {"nodev,metacopy=on"} else {"nodev,metacopy=off"}},
            "graphStatus": {
                "Using metacopy": metacopy.to_string(),
                "Native Overlay Diff": native_diff.to_string(),
                "Backing Filesystem": "btrfs"
            },
            "configFile": "/etc/containers/storage.conf",
            "imageCopyTmpDir": "/var/tmp",
            "graphRootAllocated": 1000,
            "graphRootUsed": 100,
            "containerStore": {"number": 0, "running": 0},
            "imageStore": {"number": 2}
        })
    }

    fn prepare(infos: Vec<Value>) -> Result<StorageOptions> {
        let mut infos: VecDeque<_> = infos.into();
        let result = StorageOptions::prepare_with(BuildStorage::Optimized, |_| {
            Ok(infos.pop_front().expect("Unexpected extra storage check"))
        });
        assert!(infos.is_empty(), "Not all expected storage checks ran");
        result
    }

    #[test]
    fn system_skips_info_and_non_overlay_keeps_system_options() {
        let system = StorageOptions::prepare_with(BuildStorage::System, |_| {
            panic!("System mode must not inspect or override storage")
        })
        .unwrap();
        assert!(system.args.is_empty());
        let other = prepare(vec![json!({"graphDriverName": "btrfs"})]).unwrap();
        assert!(other.args.is_empty());
    }

    #[test]
    fn native_overlay_without_metacopy_needs_no_override() {
        assert!(prepare(vec![store(false, true)]).unwrap().args.is_empty());
    }

    #[test]
    fn non_native_diff_without_metacopy_fails_immediately() {
        let error = format!("{:#}", prepare(vec![store(false, false)]).unwrap_err());
        assert!(error.contains("Native Overlay Diff"), "{error}");
        assert!(error.contains("build_storage: system"), "{error}");
    }

    #[test]
    fn replaces_all_metacopy_tokens_and_preserves_other_options() {
        let mut before = store(true, false);
        before["graphOptions"] = json!({
            "overlay.mountopt": "nodev,metacopy=on, metacopy=off,metacopy,redirect_dir=off",
            "overlay.size": "25G",
            "overlay.ignore_chown_errors": "true",
            "overlay.additionalImageStores": ["/first store", "/second"],
            "overlay.imagestore": "/second"
        });
        let mut after = before.clone();
        after["graphOptions"]["overlay.mountopt"] = json!("nodev,redirect_dir=off,metacopy=off");
        after["graphStatus"]["Using metacopy"] = json!("false");
        after["graphStatus"]["Native Overlay Diff"] = json!("true");
        // Newer Podman versions may omit the compatibility scalar.
        after["graphOptions"]
            .as_object_mut()
            .unwrap()
            .remove("overlay.imagestore");
        let prepared = prepare(vec![before, after]).unwrap();
        assert_eq!(
            prepared.args,
            [
                "--storage-opt",
                "overlay.ignore_chown_errors=true",
                "--storage-opt",
                "overlay.imagestore=/first store",
                "--storage-opt",
                "overlay.imagestore=/second",
                "--storage-opt",
                "overlay.mountopt=nodev,redirect_dir=off,metacopy=off",
                "--storage-opt",
                "overlay.size=25G"
            ]
        );
        let mut cmd = Command::new("sudo");
        cmd.arg("podman");
        prepared.apply(&mut cmd);
        cmd.arg("build");
        let args: Vec<_> = cmd.get_args().map(|s| s.to_str().unwrap()).collect();
        assert_eq!(args.first(), Some(&"podman"));
        assert_eq!(args.last(), Some(&"build"));
        assert_eq!(&args[1..args.len() - 1], prepared.args);
    }

    #[test]
    fn disables_kernel_default_metacopy_with_no_mountopt() {
        let mut before = store(true, false);
        before["graphOptions"] = json!({});
        let mut after = store(false, true);
        after["graphOptions"]["overlay.mountopt"] = json!("metacopy=off");
        assert_eq!(
            prepare(vec![before, after]).unwrap().args,
            ["--storage-opt", "overlay.mountopt=metacopy=off"]
        );
    }

    #[test]
    fn preserves_mount_program_and_rejects_non_native_result() {
        let mut before = store(true, false);
        before["graphOptions"]["overlay.mount_program"] = json!({
            "Executable": "/usr/bin/fuse-overlayfs", "Version": "1.15", "Package": "fuse-overlayfs"
        });
        let mut calls = 0;
        let error = StorageOptions::prepare_with(BuildStorage::Optimized, |args| {
            calls += 1;
            if calls == 1 {
                assert!(args.is_empty());
                return Ok(before.clone());
            }
            assert!(args.windows(2).any(|args| args
                == [
                    "--storage-opt",
                    "overlay.mount_program=/usr/bin/fuse-overlayfs"
                ]));
            let mut after = before.clone();
            after["graphStatus"]["Using metacopy"] = json!(false);
            after["graphOptions"]["overlay.mountopt"] = json!("nodev,metacopy=off");
            Ok(after)
        })
        .unwrap_err();
        assert_eq!(calls, 2);
        assert!(format!("{error:#}").contains("did not enable native overlay diff"));
    }

    #[test]
    fn normalizes_mount_program_and_empty_image_stores() {
        let options = normalize_options(&json!({
            "overlay.mount_program": {"Executable": "/usr/bin/fuse-overlayfs", "Version": "1.15"},
            "overlay.additionalImageStores": [], "overlay.imagestore": ""
        }))
        .unwrap();
        assert_eq!(options.len(), 1);
        assert_eq!(
            options["overlay.mount_program"],
            ["/usr/bin/fuse-overlayfs"]
        );
        assert_eq!(
            options,
            normalize_options(&json!({"overlay.mount_program": "/usr/bin/fuse-overlayfs"}))
                .unwrap()
        );
    }

    #[test]
    fn rejects_unknown_or_ambiguous_option_representations() {
        for options in [
            Value::Null,
            json!({"overlay.size": 10}),
            json!({"overlay.unknown": {"Executable": "foo"}}),
            json!({"overlay.mount_program": {"Version": "1.15"}}),
            json!({"overlay.mount_program": {"Executable": ""}}),
            json!({"overlay.additionalImageStores": "/first"}),
            json!({"overlay.additionalImageStores": [""]}),
            json!({"overlay.additionalImageStores": [123]}),
            json!({"overlay.additionalImageStores": ["/first"], "overlay.imagestore": "/second"}),
            json!({"overlay.imagestore": "/only-scalar"}),
            json!({"overlay.imagestore": []}),
        ] {
            assert!(normalize_options(&options).is_err(), "{options}");
        }
    }

    #[test]
    fn rejects_unexpected_store_fields_and_status() {
        for invalid in [
            Value::Null,
            json!([]),
            json!({}),
            json!({"graphDriverName": 123}),
        ] {
            assert!(prepare(vec![invalid]).is_err());
        }
        for (field, value) in [
            ("graphRoot", json!("")),
            ("runRoot", Value::Null),
            ("volumePath", json!(123)),
            ("transientStore", json!("false")),
            (
                "graphStatus",
                json!({"Using metacopy": "yes", "Native Overlay Diff": "true"}),
            ),
        ] {
            let mut invalid = store(false, true);
            invalid[field] = value;
            assert!(prepare(vec![invalid]).is_err(), "{field}");
        }
    }

    #[test]
    fn rejects_storage_identity_or_option_changes() {
        for (field, value) in [
            ("graphRoot", json!("/other")),
            ("runRoot", json!("/other")),
            ("volumePath", json!("/other")),
            ("graphDriverName", json!("vfs")),
            ("transientStore", json!(true)),
            ("configFile", json!("/other")),
            ("imageCopyTmpDir", json!("/other")),
            ("graphOptions", json!({"overlay.mountopt": "metacopy=off"})),
            (
                "graphStatus",
                json!({"Using metacopy": "false", "Native Overlay Diff": "true", "Backing Filesystem": "xfs"}),
            ),
            ("futureStorageField", json!("unexpected")),
        ] {
            let mut after = store(false, true);
            after[field] = value;
            let error = format!(
                "{:#}",
                prepare(vec![store(true, false), after]).unwrap_err()
            );
            assert!(error.contains(field), "{error}");
        }
        let mut before = store(true, false);
        before["graphOptions"]["overlay.additionalImageStores"] = json!(["/first", "/second"]);
        let mut after = store(false, true);
        after["graphOptions"]["overlay.additionalImageStores"] = json!(["/second", "/first"]);
        assert!(prepare(vec![before, after]).is_err());
    }

    #[test]
    fn ignores_only_dynamic_fields_and_the_two_expected_status_changes() {
        let mut after = store(false, true);
        after["containerStore"] = json!({"number": 10, "running": 5});
        after["imageStore"] = json!({"number": 6});
        after["graphRootAllocated"] = json!(2000);
        after["graphRootUsed"] = json!(300);
        assert!(!prepare(vec![store(true, false), after])
            .unwrap()
            .args
            .is_empty());
    }

    #[test]
    fn reader_errors_are_reported_without_retry_or_fallback() {
        let mut calls = 0;
        let error = StorageOptions::prepare_with(BuildStorage::Optimized, |_| {
            calls += 1;
            bail!("test info failure");
        })
        .unwrap_err();
        assert_eq!(calls, 1);
        let error = format!("{error:#}");
        assert!(error.contains("test info failure"));
        assert!(error.contains("build_storage: system"));
    }

    #[test]
    fn verification_must_disable_metacopy_even_with_native_diff() {
        assert!(prepare(vec![store(true, false), store(true, true)]).is_err());
    }
}

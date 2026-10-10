#![cfg(unix)]

use serde_json::{json, Value};
use std::fs;
use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::path::PathBuf;
use std::process::{Command, Output, Stdio};
use std::sync::atomic::{AtomicUsize, Ordering};

static NEXT_PROJECT: AtomicUsize = AtomicUsize::new(0);

struct TestProject {
    root: PathBuf,
}

impl TestProject {
    fn new() -> Self {
        let root = std::env::temp_dir().join(format!(
            "dcli-storage-test-{}-{}",
            std::process::id(),
            NEXT_PROJECT.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir(&root).unwrap();
        for dir in ["project", "project/hosts", "bin", "config"] {
            fs::create_dir(root.join(dir)).unwrap();
        }
        let project = Self { root };
        project.write("project/.dcli-bootc", "");
        project.profile("");
        project.write("before.json", &store(true, false).to_string());
        project.write("after.json", &store(false, true).to_string());
        // PATH is replaced only for the child process. No real sudo, Podman,
        // registry, image build, or deployment command can be invoked here.
        project.write("bin/sudo", r#"#!/bin/sh
printf '%s\n' "$*" >> "$DCLI_TEST_ROOT/commands.log"
if [ "$1" = skopeo ]; then
    [ "$DCLI_TEST_STALE" = 1 ] || exit 1
    printf '{}'
    exit 0
fi
[ "$1" = podman ] || exit 98
shift
optimized=false
while [ "$1" = --storage-opt ]; do
    optimized=true
    shift 2
done
case "$1" in
    info)
        if [ "$DCLI_TEST_STDIN" = 1 ]; then
            read -r answer
            printf 'info-stdin:%s\n' "$answer" >> "$DCLI_TEST_ROOT/commands.log"
        fi
        if [ "$DCLI_TEST_INFO_ERROR" = 1 ]; then
            printf 'mock storage unavailable\n' >&2
            exit 12
        fi
        if [ "$optimized" = true ]; then
            /bin/cat "$DCLI_TEST_ROOT/after.json"
        else
            /bin/cat "$DCLI_TEST_ROOT/before.json"
        fi
        ;;
    image)
        [ "$2" = inspect ] || exit 1
        printf 'sha256:local\n'
        ;;
    pull)
        if [ -e "$DCLI_TEST_ROOT/project/Containerfile" ]; then
            printf 'generated-before-pull\n' >> "$DCLI_TEST_ROOT/commands.log"
        fi
        if [ "$DCLI_TEST_CHANGE_PROFILE" = 1 ]; then
            printf 'name: changed\nbuild_storage: invalid\n' > "$DCLI_TEST_ROOT/project/profile.yaml"
            printf 'build_storage: invalid\n' > "$DCLI_TEST_ROOT/project/hosts/laptop.yaml"
        fi
        ;;
    build)
        # Deliberately stop at the build boundary, before any host side effects.
        exit 17
        ;;
    *) exit 98 ;;
esac
"#);
        fs::set_permissions(
            project.root.join("bin/sudo"),
            fs::Permissions::from_mode(0o755),
        )
        .unwrap();
        project
    }

    fn write(&self, path: &str, text: &str) {
        fs::write(self.root.join(path), text).unwrap();
    }

    fn profile(&self, extra: &str) {
        self.write(
            "project/profile.yaml",
            &format!(
            "name: test\nbase_image: registry.example/base:stable\nenabled_modules: []\n{extra}"
        ),
        );
    }

    fn run(&self, command: &str, args: &[&str], env: &[(&str, &str)]) -> Output {
        let mut cmd = Command::new(env!("CARGO_BIN_EXE_dcli-bootc"));
        cmd.arg("--project")
            .arg(self.root.join("project"))
            .arg(command)
            .args(["--no-import", "--no-switch"])
            .args(args)
            .env("PATH", self.root.join("bin"))
            .env("XDG_CONFIG_HOME", self.root.join("config"))
            .env("DCLI_TEST_ROOT", &self.root)
            .envs(env.iter().copied())
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped());
        let mut child = cmd.spawn().unwrap();
        child.stdin.take().unwrap().write_all(b"y\n").unwrap();
        child.wait_with_output().unwrap()
    }

    fn commands(&self) -> Vec<String> {
        fs::read_to_string(self.root.join("commands.log"))
            .unwrap_or_default()
            .lines()
            .map(str::to_string)
            .collect()
    }

    fn assert_preflight_failure(&self, output: &Output, expected: &str, reads: usize) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(stderr.contains(expected), "{stderr}");
        assert!(stderr.contains("build_storage: system"), "{stderr}");
        let commands = self.commands();
        assert_eq!(commands.len(), reads, "{commands:?}");
        assert!(commands.iter().all(|cmd| cmd.contains(" info --format ")));
        assert!(!self.root.join("project/Containerfile").exists());
        assert!(!self.root.join("project/build_files").exists());
    }

    fn assert_build_reached(&self, output: &Output, optimized: bool, reads: usize, no_cache: bool) {
        let stderr = String::from_utf8_lossy(&output.stderr);
        assert!(!output.status.success());
        assert!(stderr.contains("Build failed"), "{stderr}");
        let commands = self.commands();
        assert_eq!(
            commands
                .iter()
                .filter(|cmd| cmd.contains(" info --format "))
                .count(),
            reads,
            "{commands:?}"
        );
        let build = commands
            .iter()
            .find(|cmd| cmd.starts_with("podman build ") || cmd.contains(" build "))
            .unwrap();
        assert_eq!(
            build.contains("--storage-opt overlay.mountopt=nodev,metacopy=off build"),
            optimized,
            "{build}"
        );
        assert_eq!(build.contains("--no-cache"), no_cache, "{build}");
        // Global overrides belong only to the verification and build commands.
        for command in &commands {
            if command.contains("--storage-opt") {
                assert!(
                    command.contains(" info --format ") || command.contains(" build "),
                    "{command}"
                );
            }
        }
        assert!(self.root.join("project/Containerfile").exists());
        assert!(self.root.join("project/build_files/build.sh").exists());
    }
}

impl Drop for TestProject {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn store(metacopy: bool, native_diff: bool) -> Value {
    json!({
        "graphDriverName": "overlay",
        "graphRoot": "/var/lib/containers/storage",
        "runRoot": "/run/containers/storage",
        "volumePath": "/var/lib/containers/storage/volumes",
        "transientStore": false,
        "graphOptions": {"overlay.mountopt": if metacopy {"nodev,metacopy=on"} else {"nodev,metacopy=off"}},
        "graphStatus": {"Using metacopy": metacopy.to_string(), "Native Overlay Diff": native_diff.to_string()}
    })
}

#[test]
fn build_and_update_fail_before_generation_registry_pull_or_build() {
    for command in ["build", "update"] {
        let project = TestProject::new();
        let output = project.run(command, &[], &[("DCLI_TEST_INFO_ERROR", "1")]);
        project.assert_preflight_failure(&output, "mock storage unavailable", 1);

        let project = TestProject::new();
        project.write("before.json", "not JSON");
        let output = project.run(command, &[], &[]);
        project.assert_preflight_failure(&output, "Invalid Store JSON", 1);

        let project = TestProject::new();
        project.write("before.json", &store(false, false).to_string());
        let output = project.run(command, &[], &[]);
        project.assert_preflight_failure(&output, "Native Overlay Diff is disabled", 1);
    }
}

#[test]
fn verification_errors_also_stop_build_and_update_early() {
    for command in ["build", "update"] {
        for (field, value, error) in [
            ("graphRoot", json!("/different-store"), "graphRoot"),
            (
                "graphStatus",
                json!({"Using metacopy": "false", "Native Overlay Diff": "false"}),
                "did not enable native overlay diff",
            ),
        ] {
            let project = TestProject::new();
            let mut after = store(false, true);
            after[field] = value;
            project.write("after.json", &after.to_string());
            let output = project.run(command, &[], &[]);
            project.assert_preflight_failure(&output, error, 2);
        }
    }
}

#[test]
fn sudo_info_can_read_inherited_stdin() {
    let project = TestProject::new();
    project.write("before.json", &store(false, false).to_string());
    let output = project.run("update", &[], &[("DCLI_TEST_STDIN", "1")]);
    assert!(String::from_utf8_lossy(&output.stderr).contains("Native Overlay Diff is disabled"));
    assert!(project.commands().iter().any(|line| line == "info-stdin:y"));
}

#[test]
fn build_uses_only_the_verified_global_options() {
    let project = TestProject::new();
    let output = project.run("build", &["--no-cache"], &[]);
    project.assert_build_reached(&output, true, 2, true);
    assert!(!project
        .commands()
        .iter()
        .any(|cmd| cmd.starts_with("podman pull ")));
}

#[test]
fn update_reuses_preparation_and_preserves_cache_policy() {
    for cache in [false, true] {
        let project = TestProject::new();
        let args = if cache { vec!["--cache"] } else { vec![] };
        let output = project.run("update", &args, &[("DCLI_TEST_CHANGE_PROFILE", "1")]);
        project.assert_build_reached(&output, true, 2, !cache);
        let commands = project.commands();
        assert_eq!(
            commands
                .iter()
                .filter(|cmd| cmd.starts_with("podman pull "))
                .count(),
            1
        );
        assert!(!commands.iter().any(|cmd| cmd == "generated-before-pull"));
    }
}

#[test]
fn stale_build_hands_off_once_with_the_same_profile_host_and_identity() {
    let project = TestProject::new();
    project.profile("base_images:\n  deck: registry.example/deck:stable\n");
    project.write("project/hosts/laptop.yaml", "build_storage: optimized\n");
    let output = project.run(
        "build",
        &[
            "--host",
            "laptop",
            "--base-image",
            "deck",
            "--tag",
            "custom",
        ],
        &[("DCLI_TEST_STALE", "1"), ("DCLI_TEST_CHANGE_PROFILE", "1")],
    );
    project.assert_build_reached(&output, true, 2, true);
    let commands = project.commands();
    assert_eq!(
        commands
            .iter()
            .filter(|cmd| cmd.starts_with("podman pull registry.example/deck:stable"))
            .count(),
        1
    );
    assert!(!commands.iter().any(|cmd| cmd == "generated-before-pull"));
    let build = commands.iter().find(|cmd| cmd.contains(" build ")).unwrap();
    assert!(
        build.contains("BASE_IMAGE=registry.example/deck:stable"),
        "{build}"
    );
    assert!(build.contains("-t project:custom"), "{build}");
    let generated = fs::read_to_string(project.root.join("project/build_files/build.sh")).unwrap();
    assert!(generated.contains("laptop"));
}

#[test]
fn system_other_drivers_and_already_native_overlay_keep_original_build_arguments() {
    for (policy, info, reads, message) in [
        (
            "build_storage: system\n",
            Value::Null,
            0,
            "builds may be slow",
        ),
        ("", json!({"graphDriverName": "btrfs"}), 1, "does not apply"),
        (
            "",
            store(false, true),
            1,
            "already uses native overlay diff",
        ),
    ] {
        let project = TestProject::new();
        project.profile(policy);
        project.write("before.json", &info.to_string());
        let output = project.run("build", &[], &[]);
        project.assert_build_reached(&output, false, reads, false);
        assert!(String::from_utf8_lossy(&output.stdout).contains(message));
    }
}

#[test]
fn host_storage_overrides_work_in_both_directions_and_absence_inherits() {
    for (profile, host, optimized) in [
        ("optimized", "build_storage: system\n", false),
        ("system", "build_storage: optimized\n", true),
        ("optimized", "{}", true),
        ("system", "{}", false),
    ] {
        let project = TestProject::new();
        project.profile(&format!("build_storage: {profile}\n"));
        project.write("project/hosts/laptop.yaml", host);
        let output = project.run("build", &["--host", "laptop"], &[]);
        project.assert_build_reached(&output, optimized, if optimized { 2 } else { 0 }, false);
    }
}

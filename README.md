# dcli-bootc

A declarative **bootc** image configuration manager. Define your system
packages, repos, files, services, and scripts in YAML modules, then generate a
Containerfile and build the image.

## Installing from COPR

### RakuOS (rum usr-overlay) — recommended

RakuOS does not need an rpm-ostree layer or a rebuilt image: its `rum`
package manager installs into the live `/usr` overlay, so `dcli-bootc` goes in
like any other package and survives image updates.

```bash
sudo rum copr enable theblackdon/dcli-bootc   # chroot auto-detected from os-release
sudo rum install dcli-bootc
```

No reboot and no image rebuild — the command is available immediately. Useful
follow-ups:

```bash
rum origin dcli-bootc      # confirm it's Overlay (not Base)
rum upgrade dcli-bootc     # update to the newest COPR build
rum remove dcli-bootc      # uninstall
```

The install is per-machine and lives in the overlay, so it is not baked into
your image and won't exist on another RakuOS box until you run the same two
commands there. If you would rather have it inside the image (so every
rebuild/rebase carries it), add it to an enabled module as in Option 1 below
instead — the overlay and image copies coexist fine.

### Atomic / bootc distros (Bazzite, Silverblue, Kinoite, Bluefin, Aurora)

`dnf install` does not work on the live host (`/usr` is read-only), and
binaries copied to `/usr/local/bin` sit outside the image lifecycle. Pick
one of these instead:

**Option 1 — bake it into your image (recommended).** Projects created with
`dcli-bootc init` do this automatically via the generated `base.yaml`. For an
existing project, add the repo and package to any enabled module:

```yaml
packages:
  - dcli-bootc
repos:
  copr:
    - theblackdon/dcli-bootc
```

Then `dcli-bootc build` and reboot. The tool now lives inside the image:
it survives every rebuild/rebase and stays current with each
`dcli-bootc update`.

**Option 2 — layer it on the running system (rpm-ostree).**

```bash
sudo curl -sSL -o /etc/yum.repos.d/_copr_theblackdon-dcli-bootc.repo   https://copr.fedorainfracloud.org/coprs/theblackdon/dcli-bootc/repo/fedora-$(rpm -E %fedora)/theblackdon-dcli-bootc-fedora-$(rpm -E %fedora).repo
sudo rpm-ostree install dcli-bootc
sudo systemctl reboot
```

Caveat: layering packages marks deployments `incompatible` in bootc — from
then on, system mutations (upgrades/switches) must go through `rpm-ostree`
instead of `bootc`. If you drive your system with `dcli-bootc`, prefer
Option 1.

**Option 3 — transient install for testing.**

```bash
sudo bootc usr-overlay
sudo dnf copr enable theblackdon/dcli-bootc
sudo dnf install dcli-bootc    # gone after the next reboot
```

## Commands

| Command | Description |
|---------|-------------|
| `init`  | Initialize a new bootc image project |
| `gen`   | Generate Containerfile and build script from YAML modules |
| `build` | Build the container image (warns when the base image is outdated) and optionally switch bootc deployment |
| `switch`| Switch the bootc deployment to the locally built image |
| `rollback` | Roll back to the previous bootc deployment |
| `update`| Pull the latest base image, rebuild, and switch to the fresh build |
| `status`| Show bootc deployment status and project info |
| `overlay` | Import / sync the RakuOS `rum` usr-overlay with a module (RakuOS hosts only) |
| `dotfiles` | Apply profile dotfiles into ~ as symlinks (also runs on build/update) |

## Quickstart

```bash
# Create a new project
dcli-bootc init my-image
cd my-image

# Edit modules and profile.yaml...
# Then generate and build
dcli-bootc gen
dcli-bootc build
```

## Project Structure

```
my-image/
├── profile.yaml        # Base image, tag, enabled modules
├── modules/            # YAML module definitions
│   ├── base.yaml
│   └── ...
├── hosts/              # Per-host overlays (optional)
│   ├── desktop.yaml
│   └── laptop.yaml
├── files/              # Files to copy into the image
├── rpms/               # Local .rpm files installed via local_rpms
├── appimages/          # Local AppImages installed via appimages
├── dotfiles/           # Dotfiles symlinked into ~ (see Dotfiles section)
└── .dcli-bootc         # Project marker
```

## Configuration

`profile.yaml` defines the base image and enabled modules. Each module in
`modules/*.yaml` can declare packages, groups, repos, remote RPMs, local RPMs,
appimages, flatpaks, files, services, and inline scripts.

## Multi-host

One project can build image variants for several machines. `profile.yaml`
holds the shared configuration; each machine gets an optional overlay in
`hosts/<name>.yaml` that adds modules on top of the profile's
`enabled_modules`, can exclude profile modules, and may override the base
images:

```yaml
# hosts/desktop.yaml
description: Gaming desktop

modules:                 # added to the profile's enabled_modules
  - gaming
  - nvidia

exclude_modules:         # removed from the profile's list (optional)
  - tlp

base_image: ghcr.io/ublue-os/bazzite-nvidia-open:stable   # optional override

# RakuOS only: use this host's own rum usr-overlay manifest instead of the
# profile's `overlay_module`. The module lives in modules/ like any other.
overlay_module: overlay-desktop
```

The active host is chosen by the global `--host <name>` flag, or
auto-detected from the machine's hostname when a matching
`hosts/<hostname>.yaml` exists (NixOS flake-style). With no match, the plain
profile is used, so projects without a `hosts/` directory are unaffected.

```bash
dcli-bootc gen --host desktop      # generate for a specific host
dcli-bootc build                   # auto-detects host from hostname
dcli-bootc build --host laptop     # explicit host
dcli-bootc status                  # shows active host + all hosts
```

Host builds are tagged with the host name (`localhost/my-image:desktop`),
so every machine's variant coexists in local container storage; an explicit
`--tag` always wins. `build` regenerates `Containerfile`/`build.sh` whenever
they drift from the modules **or the active host**, and the host is recorded
in the `build.sh` header — so switching hosts always rebuilds the
customization layer instead of reusing a stale cached one. `switch` and
`update` understand `--host` too.

### Local RPMs

When a download is slow or unreliable during builds (vendor CDNs, gated
downloads), fetch the `.rpm` yourself, drop it in the project's `rpms/`
directory, and declare it by filename in a module:

```yaml
local_rpms:
  - Lightworks-2025.2-56356.rpm
```

`gen` fails fast if a declared file is missing from `rpms/`, instead of the
build dying halfway through. The files are bind-mounted into the build and
installed with `dnf install` — they never land in an image layer. Swapping a
file in `rpms/` invalidates the customization layer via the build-input hash,
and the post-build verification confirms each local RPM's package is actually
installed. The `.gitignore` written by `init` already excludes `*.rpm`, so the
binaries stay out of git.

### dnf Groups

Install named package groups (fedora groups like `standard`,
`development`, `gnome-desktop`) with `dnf group install`:

```yaml
groups:
  - development
  - standard
```

Groups only apply when the profile's `package_manager` is `dnf`; they are
ignored for other package managers. Installed group members are verified like
regular packages after the build.

### AppImages

Drop AppImage binaries in the project's `appimages/` directory and declare
them by filename in a module:

```yaml
# modules/apps.yaml
appimages:
  - Obsidian.AppImage
  - Signal.AppImage
```

At build time each AppImage is installed to `/usr/local/bin/<name>` (making it
executable and on `PATH`), and its desktop integration is extracted
automatically: the `.desktop` file is copied to `/usr/share/applications/` and
icons to `/usr/share/icons/hicolor/`. Extraction uses `--appimage-extract`, so
no FUSE is needed in the build.

`gen` fails fast if a declared file is missing from `appimages/`, the files
are bind-mounted into the build (never baked into an image layer), changes to
`appimages/` bust the build cache via the build-input hash, and post-build
verification confirms each AppImage landed in `/usr/local/bin`. The
`.gitignore` written by `init` excludes `*.AppImage`/`*.appimage`.

### Flatpaks

Flatpaks are installed **system-wide into the image** at build time (like
packages), so they ship with the image and survive rebases:

```yaml
# modules/apps.yaml
flatpaks:
  - org.mozilla.firefox
  - com.obsproject.Studio
remove_flatpaks:          # remove apps baked into the base image
  - org.fedoraproject.MediaWriter
```

Installs come from the first configured remote — flathub by default, so no
remote config is needed. To use another remote (or add flathub explicitly),
declare remotes in any module; they are merged and deduplicated by name
across all modules and added before any installs:

```yaml
flatpak_remotes:
  - name: flathub
    url: https://dl.flathub.org/repo/flathub.flatpakrepo
  - name: fedora
    url: https://dl.fedoraproject.org/pub/fedora/linux/stable/41/x86_64/flatpak-apps/
```

The generated `build.sh` runs `flatpak remote-add --if-not-exists --system`
for each remote, then `flatpak install -y --or-update --system <remote>
<app>` per module (deduplicated across modules; apps already baked into the
base image are updated instead of failing) and
`flatpak uninstall -y --system` for removals. The build plan lists them, and post-build verification boots
the image and confirms each declared flatpak is present and each removal is
gone — skipped with a warning when the base image has no `flatpak` binary.
The base image must ship `flatpak` (bazzite and other desktop bases do);
user-level flatpaks installed on the running system are not affected.

## Kernel management

`profile.yaml` can declare which kernel the image ships — for example a
CachyOS or LTS kernel from a COPR instead of the base image's stock kernel:

```yaml
# profile.yaml
kernel:
  repo:
    copr: [bieszczaders/kernel-cachyos]
  packages:
    - kernel-cachyos
  version: "6.11.5"     # optional; pins the install spec
  versionlock: true     # optional; dnf versionlock the installed kernel
  regen_initramfs: true # default; required for the kernel to boot
```

Kernel management is **dnf-only** and `gen` fails fast for other package
managers. The generated `build.sh` emits this section before any module so the
kernel is settled first:

1. enable the declared repo(s) and import keys;
2. remove the stock kernel — by default the Fedora set (`kernel`,
   `kernel-core`, `kernel-modules`, `kernel-modules-core`,
   `kernel-modules-extra`), but only packages that are actually installed, so
   the default is safe on base images that name things differently. Set
   `remove: []` to keep the stock kernel, or list packages explicitly;
3. install `packages` (with `-<version>` appended when `version` is set);
4. `dnf versionlock` the installed kernel when `versionlock: true` (fails
   fast if the plugin is missing);
5. `depmod` + `dracut` the single remaining kernel to
   `/usr/lib/modules/<kver>/initramfs.img`.

bootc supports exactly one kernel per image — `bootc container lint`'s
`kernel` check is fatal otherwise — so removing the stock kernel is part of
the contract. The post-build verification blocks the switch prompt unless the
image has exactly one kernel with both `vmlinuz` and `initramfs.img`, and (when
`version` is set) the installed version matches the pin.

### Multi-device kernels

Each host can override the profile's `kernel:` block wholesale, exactly like
`base_image`:

```yaml
# hosts/laptop.yaml
kernel:
  repo:
    copr: [bieszczaders/kernel-cachyos]
  packages: [kernel-cachyos-lts]
  version: "6.11.5"
  versionlock: true
```

The override flows through `--host`/hostname auto-detection like everything
else: each device generates its own `build.sh`, gets its own content hash and
`localhost/<image>:<host>` tag, and is verified independently.

### Pinning caveats

`version`/`versionlock` pin within the build, but a `bootc upgrade` rebases
the image and does not run dnf — to freeze the kernel across rebases, pin the
**base image** by digest (`base_image: …@sha256:…`) as well. Pinning only
works while the kernel repo still carries that version.

## RakuOS (rum usr-overlay)

RakuOS mounts a persistent overlayfs over `/usr`, and its `rum` package
manager installs packages into it live (`rum install`). That is the point of
RakuOS: they are installed in real time, survive image updates, and are not
baked into the image. dcli-bootc keeps them that way — it just makes the set
declarative instead of clicking through the software center. (Install
`dcli-bootc` itself with `rum` — see
[Installing from COPR](#rakuos-rum-usr-overlay--recommended).)

Point the profile at a module that holds the overlay manifest. Do **not** add
it to `enabled_modules` — its packages belong in the overlay, not the image:

```yaml
# profile.yaml
enabled_modules:
  - base
overlay_module: overlay      # NOT enabled_modules — rum-managed, overlay-only
```

For a project that builds for several machines, give each `hosts/<name>.yaml`
its own `overlay_module` so the two live overlays never reconcile against each
other (each machine has a separate `/var/lib/rakuos/packages.list`; only the
manifest is shared). A host without `overlay_module` falls back to the
profile's:

```yaml
# hosts/desktop.yaml
overlay_module: overlay-desktop
# hosts/laptop.yaml
overlay_module: overlay-laptop
```

Then `overlay import` / `sync` / `status` / `prune` / `clean` act on the active
host's module (auto-detected from the hostname, or chosen with `--host`).

Then snapshot what `rum` has installed into that module:

```bash
# writes the overlay-installed packages into modules/overlay.yaml
dcli-bootc overlay import
```

`import` reads `/var/lib/rakuos/packages.list` (the packages you asked `rum`
for, not the dependency closure) and merges them into the module, preserving
any other keys already there (`description`, `scripts`, ...). It refuses to
clobber a module that already declares packages unless you pass `--force`.

From then on the module is the manifest for the live overlay. `build`,
`update`, and `overlay sync` reconcile the host overlay with `rum` — no
software center, no manual `rum` commands:

* packages **added to the module** are `rum install`ed into the overlay;
* packages **removed from the module** are `rum remove`d from the overlay —
  except packages the base image provides (declared by `enabled_modules` or
  already present in the booted image), which are **kept**. Removing those
  with `rum` would unlink through the merged `/usr` and leave an overlayfs
  whiteout hiding the base image's copy, so `sync` leaves them in place and
  reports them. To shed such a duplicate cleanly, drop it from the overlay
  module/`packages.list` and run `rakuos reset-overlay --soft`.

```bash
dcli-bootc overlay status          # what would be installed / removed
dcli-bootc overlay sync            # apply it now, without rebuilding the image
dcli-bootc overlay sync --dry-run  # show what would change
```

`build` and `update` run the same sync after a successful image build, so a
rebuild also brings the live overlay in line with the manifest.

Once a package has been baked into *and booted from* the base image it no
longer needs to live in the overlay module. `prune` drops those:

```bash
dcli-bootc overlay prune            # drop base-provided packages from the module
dcli-bootc overlay prune --dry-run  # show what would be dropped
```

It counts only packages the **currently booted** base image provides (queried
from its own rpmdb), never one that is merely declared in `enabled_modules` but
hasn't booted yet — so it can't prune something you don't actually have. The
live overlay copies are left untouched (removing them with `rum` would hide the
base image's copies with overlayfs whiteouts); run `rakuos reset-overlay
--soft` afterwards to reclaim the overlay space.

Locally sideloaded RPMs tracked in `/var/lib/rakuos/packages-rpm.list` cannot
be reproduced from a repo; `import` prints them so you can declare them as
`local_rpms`/`remote_rpms` instead (see [Local RPMs](#local-rpms)).

These commands are no-ops on hosts without `rum`, and on projects without
`overlay_module`, so non-RakuOS projects are unaffected. If you do add the
manifest module to `enabled_modules`, `gen`/`build` warn: its packages would
then be baked into the image as well as installed in the overlay.

## Dotfiles

## Dotfiles

Drop per-app config directories or files into the project's `dotfiles/`
directory and list them in `profile.yaml`:

```
dotfiles/
├── fish/
├── helix/
└── bashrc
```

```yaml
# profile.yaml
dotfiles:
  - fish/              # → ~/.config/fish
  - helix/             # → ~/.config/helix
  - bashrc: ~/.bashrc  # custom target for files outside ~/.config
```

Plain entries are symlinked to `~/.config/<name>`; use the single-key map
form (`source: target`, with a `~/` or absolute path) for files that live
elsewhere.

Dotfiles are applied **host-side** — container builds run as root and cannot
touch user homes — at the end of `dcli-bootc build`/`update`, or on demand
without building:

```bash
dcli-bootc dotfiles
```

Each target becomes a symlink into the project
(`~/.config/fish` → `<project>/dotfiles/fish`), so changes apps write to
`~/.config` land in the project and survive image rebuilds — commit them to
git. Applying is idempotent: correct symlinks are skipped, wrong ones
re-pointed, and existing real files/dirs are moved to `<target>.dcli-bak`
before linking, so nothing is lost. Declared entries must exist in
`dotfiles/` — `gen` and `build` fail fast otherwise (same as `local_rpms`).

Hosts can add or remove dotfiles exactly like modules (`exclude_dotfiles`
matches by source name):

```yaml
# hosts/laptop.yaml
dotfiles:
  - yazi/
exclude_dotfiles:
  - helix
```

Caveats: whole-directory symlinks mean app state written next to your config
(e.g. fish's `fish_variables`) also lands in the project — gitignore what you
don't want to track. Apps that rewrite a config *file* via
write-temp-then-rename can replace its symlink with a real file; re-running
`dcli-bootc dotfiles` repairs it.

## Build

```bash
dcli-bootc build             # Build and tag as latest
dcli-bootc build --nvidia    # Use NVIDIA base image variant
dcli-bootc build --no-cache  # Disable podman layer cache entirely
dcli-bootc build --reboot    # Reboot after switching
```

`build` automatically re-generates `Containerfile` and `build_files/build.sh`
if they have drifted from your modules, so a build always reflects the current
configuration. Package installs run without `--skip-unavailable`, so an
unavailable package fails the build instead of being silently dropped.

Before any work happens, `build` and `update` print a **build plan**: every
module's repos, removals, package installs, remote and local RPMs (with their
real package names and sizes), file overlays, and services — then a comparison
against the previously built image showing which declared packages are new
since then, and which declared removals are actually present there. The
comparison is best-effort: it is skipped when no previous build exists or its
package list can't be read. `update` prints the plan before pulling and also
reports whether the pull actually changed the base image.

`build` works on your **local copy** of the base image — it never pulls OS or
base-image updates itself. Before building, it checks whether the registry has
a newer base image; when yours is outdated, it warns you and offers to switch
to `update` (pull the latest base + rebuild fresh) instead. `--force` skips
the question but still prints the warning. The check is best-effort: it needs
network access and `skopeo`, and is skipped when either is unavailable.

After building, dcli-bootc boots the image once to verify that every package
declared in your modules is actually installed (and that every removed
package is gone). If anything is off, it tells you exactly what and does
**not** offer to switch to that image.

### Cache safety

The whole customization runs as a single `RUN` layer with the build files
bind-mounted in. Bind-mounted content is **not** part of podman's layer cache
key, so a naive Containerfile can silently reuse a stale layer even after you
change your modules — the build "succeeds" but the image never changes.

dcli-bootc prevents this: it hashes everything under `build_files/` and
`files/` and passes the hash as the `DCLI_BUILD_HASH` build arg, which is
referenced by the `RUN` instruction. Any input change rebuilds exactly that
layer (base image layers stay cached). Use `--no-cache` to force a full
rebuild.

## Switching and applying

Answering `y` at the build's switch prompt runs
`sudo bootc switch --transport containers-storage localhost/<name>:<tag>`
and then **verifies** what bootc staged, printing the staged image and digest
so you can be sure the change actually landed. When the system already tracks
that exact image, `bootc switch` is a no-op ("Image specification is
unchanged") — dcli-bootc detects this and runs `sudo bootc upgrade` instead,
which stages the freshly built digest of the tracked image. The new deployment only takes
effect **after a reboot** — you'll be asked whether to reboot right away
(`--reboot` skips the question and reboots automatically). If nothing new was
staged, no reboot is offered.

```bash
dcli-bootc switch            # Switch to the locally built image later
dcli-bootc switch --reboot   # Switch and reboot immediately
dcli-bootc status            # Show tracked/booted/staged images and digests
```

## Update

```bash
dcli-bootc update            # Pull latest base image, rebuild, and switch
dcli-bootc update --nvidia   # Update using the NVIDIA base image variant
dcli-bootc update --reboot   # Same, then reboot
```

This is the flow `dcli-bootc build` offers to hand off to when it detects that
your local base image is outdated.

## Rollback

```bash
dcli-bootc rollback            # Queue the previous deployment for next boot
dcli-bootc rollback --reboot   # Roll back and reboot immediately
```

This wraps `sudo bootc rollback`: it shows the booted and rollback
deployments first and asks for confirmation (`--force` skips it). On the
next boot the previous deployment runs and the current one becomes the new
rollback target.

**Mind the staged deployment.** bootc keeps only two deployments (booted +
rollback) plus one staged update. If an update is staged but not yet booted,
rolling back **discards it** and boots the rollback deployment — you end up
further back than just undoing the pending update. dcli-bootc warns loudly
in this case; to cancel only the pending update, remove just the staged
entry (`sudo ostree admin status`, then `sudo ostree admin undeploy 0`).

**Tags vs. digests.** Every local build reuses the same tag
(`localhost/<name>:<tag>`), so deployments are told apart by **digest**, not
tag — `dcli-bootc status` shows each deployment's digest and **build date**
(as long as the image is still in container storage). Rolling back twice in
a row toggles between the two most recent deployments. To go forward to your
newest local build again, run `dcli-bootc switch` — the tag always points at
the latest build in container storage, and switching re-stages it. To boot
an older image that is no longer a deployment, re-tag its digest and switch:
`sudo podman tag sha256:<digest> localhost/<name>:rescue` then
`dcli-bootc switch --tag rescue`.

Two caveats come from bootc itself: `/etc` changes made since the old
deployment do not carry over (bootc reorders existing deployments instead of
creating a new one, so `/etc` reverts to the rolled-back state), and an
auto-update agent such as `bootc-fetch-apply-updates.timer` may re-apply
updates and revert the rollback.

Rollback is a purely system-level operation: it works from any directory (no
project needed) and never touches profiles, hosts, or tags — the deployments
bootc keeps already record which image (including which host variant) they
boot into.

## Packaging

`dcli-bootc.spec` plus `.copr/Makefile` produce a COPR-ready SRPM
(`make srpm` locally, or COPR's **make srpm** SCM method automatically):

```bash
make srpm        # writes dist/dcli-bootc-<version>-1.src.rpm
```

The SRPM step archives the checked-out tree (on COPR: the pushed revision —
commit and push before building for release). Crates are downloaded from crates.io during the RPM build, so the
COPR project needs **"Enable network during builds"** turned on in its
settings. COPR package settings: SCM source
`https://gitlab.com/theblackdon/dcli-bootc.git`, build method **make srpm**,
spec path `dcli-bootc.spec`.

For development installs, `./build-dev.sh` builds the same RPM locally and
offers to install the packaged binary to `~/.local/bin` — this works on
atomic hosts where `/usr` is read-only. (`./build-dev.sh --system` installs
to `/usr/local/bin` with sudo for regular Fedora.)

## License

0BSD

# dcli-bootc

A declarative **bootc** image configuration manager. Define your system
packages, repos, files, services, and scripts in YAML modules, then generate a
Containerfile and build the image.

The package also installs a short `dcli` alias, so `dcli build`, `dcli update`,
and the rest work exactly like `dcli-bootc ...`.

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
| `overlay` | Import / sync / upgrade the host usr-overlay with a module (RakuOS `rum` or kineticos `kinpkg`) |
| `dotfiles` | Apply profile dotfiles into ~ as symlinks (also runs on build/update) |
| `completions` | Generate shell completion scripts (bash, fish, zsh) |

Every command is also available through the short `dcli` alias (`dcli update`,
`dcli overlay status`, ...).

### Shell completions

Installing the RPM sets up tab-completion for **bash**, **fish**, and **zsh**
automatically, for both `dcli-bootc` and the `dcli` alias. For a source/dev build
you can generate the scripts yourself:

```bash
# bash
dcli-bootc completions bash > ~/.local/share/bash-completion/completions/dcli-bootc
# fish
dcli-bootc completions fish > ~/.config/fish/completions/dcli-bootc.fish
# zsh
dcli-bootc completions zsh  > ~/.local/share/zsh/site-functions/_dcli-bootc
```

Add `--bin dcli` to generate completions for the short alias instead.
`./build-dev.sh` installs user-level completions for you.

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

On **kineticos**, bare `init` asks how you want to use the image:

* **Extend** (recommended) — add your own packages/modules on top. The project's
  `base_image` is the kineticos image the system tracks, so kineticos keeps
  managing the base and its updates flow through automatically. The full
  upstream recipe is readable at `/usr/share/kineticos/project/`; you don't edit
  it. The project gets its own `modules/overlay-packages.yaml` (seeded from the
  live usr-overlay, empty if there is nothing installed yet), so your overlay
  packages carry over and stay editable.
* **Fork** — copy the complete kineticos project for full control. `base_image`
  stays the template's base (Bazzite), so you take over upstream updates.

```bash
dcli-bootc init my-image              # kineticos: prompted Extend/Fork
dcli-bootc init --extend my-image     # layer on the running image
dcli-bootc init --fork my-image       # full copy for full control
dcli-bootc init --from-image my-image # generic layer on any bootc host
dcli-bootc init --template <dir> my-image
```

Non-interactive `init` on kineticos defaults to **Extend**. `--from-image` sets
`base_image` to the image the system tracks (stripped of any transport
prefix/digest), so the project layers on the upstream image and keeps receiving
its updates; `--nvidia` falls back to that base. The template is resolved from
`--template`, then `/usr/share/kineticos/project`, then `~/.config/kineticos`.

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

Set `include_project: true` in `profile.yaml` to bake the project's own source
into the image at `/usr/share/<name>/project/`. That is what lets a running
system reproduce or fork its recipe offline via `dcli-bootc init`. The snapshot
excludes `.git`, `.dcli-bootc`, `build_files/`, `output/`, and `target/`.

### Base-image variants

`profile.yaml` can declare named base images beyond `base_image` (the
default/AMD-Intel image). Select one at build time with `--base-image <name>`:

```yaml
base_image: ghcr.io/ublue-os/bazzite:stable          # default (AMD/Intel)

base_images:
  nvidia: ghcr.io/ublue-os/bazzite-nvidia-open:stable
  nvidia-legacy: ghcr.io/ublue-os/bazzite-nvidia:stable
  deck: ghcr.io/ublue-os/bazzite-deck:stable
```

```bash
dcli-bootc build  --base-image nvidia-legacy   # tagged :nvidia-legacy
dcli-bootc update --base-image deck            # tagged :deck
```

A named variant defaults its tag to the variant name, so `--base-image deck`
alone builds `:deck` (no `--tag` needed) and variants never overwrite the
`default_tag` image; an explicit `--tag` still wins. The selected image is
passed to the build as `--build-arg BASE_IMAGE=...`, so the generated
`Containerfile` keeps the profile's declared `base_image` as its default.

`--nvidia` remains as an alias for `--base-image nvidia`. For backwards
compatibility, `nvidia` falls back to the legacy `nvidia_base_image:` field when
`base_images` has no `nvidia` entry — prefer `base_images`, which is what lets a
variant live entirely in `profile.yaml` instead of a separate `hosts/` file.

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

# usr-overlay only: use this host's own overlay manifest instead of the
# profile's `overlay_module`. The module lives in modules/ like any other.
overlay_module: overlay-desktop
overlay_backend: auto        # auto | rum | kinpkg (optional)
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

On atomic/bootc bases (Bazzite, Silverblue, KineticOS, …) `/opt` is a symlink
into `/var`, which is machine-local state and is not part of the deployed
image. `dcli-bootc` stages `/opt` under `/var` before installing, then promotes
it to a real `/opt` directory in the image, so RPMs that ship their payload
under `/opt` persist on the running system instead of vanishing. The same
handling applies to `remote_rpms`. On non-atomic bases `/opt` is a real
directory already, so this is a no-op.

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

On atomic/bootc bases `/usr/local` is a symlink into `/var`, so an AppImage
installed there would otherwise vanish on the running system. `dcli-bootc`
promotes `/usr/local` to a real directory in the image, keeping the AppImages
installed after deployment. On non-atomic bases `/usr/local` is already a real
directory, so this is a no-op.

### Flatpaks

Flatpaks are installed into the **invoking user's** flatpak installation
(`flatpak --user`) when you run `dcli-bootc build`/`update`, and managed
independently with `dcli-bootc flatpak sync` / `status`.

They are deliberately **not** baked into the image. A system flatpak lives in
`/var/lib/flatpak`, which under bootc is machine-local state: a
`bootc switch`/`upgrade` keeps the running system's `/var` and only swaps
`/usr`, so anything installed there at build time would not survive a rebase.
Keeping flatpaks user-scoped puts them in `$HOME`, where they persist normally.

```yaml
# modules/apps.yaml
flatpaks:                 # alias: user_flatpaks
  - org.mozilla.firefox
  - com.obsproject.Studio
remove_flatpaks:          # alias: user_remove_flatpaks
  - org.fedoraproject.MediaWriter
```

`flatpaks:`/`remove_flatpaks:` are aliases for `user_flatpaks:`/
`user_remove_flatpaks:` and may be mixed freely; all four are merged and
deduplicated across enabled modules.

Installs come from the first configured remote — flathub by default, so no
remote config is needed. To use another remote (or add flathub explicitly),
declare remotes in any module; they are merged and deduplicated by name:

```yaml
flatpak_remotes:          # alias: user_flatpak_remotes
  - name: flathub
    url: https://dl.flathub.org/repo/flathub.flatpakrepo
  - name: fedora
    url: https://dl.fedoraproject.org/pub/fedora/linux/stable/41/x86_64/flatpak-apps/
```

The build plan lists the remotes, installs, and removals. The sync runs as the
invoking user (never via sudo) and prompts before changing anything; pass
`--force` to skip the prompt, or use `dcli-bootc flatpak sync --dry-run` to
preview. `dcli-bootc flatpak status` shows drift between the declared set and
your live user installation. The running system must have the `flatpak` binary
(bazzite and other desktop bases ship it); when it is absent the sync is a
no-op.

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

## Persistent usr-overlay (RakuOS rum / kineticos kinpkg)

Some atomic images mount a persistent overlayfs over `/usr` so a native
package manager can install packages into it live — they survive image
updates and are not baked into the image. dcli-bootc keeps them that way; it
just makes the set declarative. Two backends are supported:

* **`rum`** — RakuOS. `rum install` writes into the overlay and records names
  in `/var/lib/rakuos/packages.list`. (Install `dcli-bootc` itself with `rum`
  — see [Installing from COPR](#rakuos-rum-usr-overlay--recommended).)
* **`kinpkg`** — kineticos. A persistent `/usr` overlay (shipped by the
  project's `overlay` module) with the `kinpkg` command as the installer,
  recorded in `/var/lib/kineticos/overlay/installed.txt`. Unlike RakuOS,
  kineticos has a single merged rpmdb, so base-provided packages are read from
  a list dcli-bootc bakes into the image at build time.

Pick one with `overlay_backend: auto | rum | kinpkg` (default `auto` — RakuOS
`rum` when present on the host, otherwise `kinpkg`).

Point the profile at a module that holds the overlay manifest. Do **not** add
it to `enabled_modules` — its packages belong in the overlay, not the image:

```yaml
# profile.yaml
enabled_modules:
  - base
overlay_module: overlay      # NOT enabled_modules — overlay-managed, overlay-only
overlay_backend: kinpkg      # optional: auto | rum | kinpkg
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

The module records the live overlay; it is not a gatekeeper. `build`/`update`
never remove a package just because the module does not list it. They install
the packages the module declares and then **snapshot** the live overlay back
into the module, so a plain `dnf`/`kinpkg` install is recorded automatically —
you never have to run `overlay import` before updating:

* packages you install live (`sudo dnf install foo` / `sudo ki install foo`)
  are kept, and `build`/`update` write them into the module for you;
* packages **added to the module** are installed into the overlay on the next
  `build`/`update` (or `overlay sync`);
* packages the base image provides are **kept** even when the module drops
  them — removing one would unlink through the merged `/usr` and leave an
  overlayfs whiteout hiding the base image's copy, so `sync` leaves it in
  place and reports it. To shed such a duplicate cleanly, drop it from the
  module/`packages.list` and run `rakuos reset-overlay --soft`.

An explicit `overlay sync` is authoritative and *does* remove packages the
module does not declare. To remove something, either uninstall it live and let
the next snapshot drop it, or run:

```bash
dcli-bootc overlay status          # what would be installed / removed
dcli-bootc overlay sync            # authoritative: install AND remove to match
dcli-bootc overlay sync --dry-run  # show what would change
```

`dcli-bootc overlay import` still exists for an explicit one-off snapshot, but
the automatic snapshot makes it unnecessary before `build`/`update`. Pass
`--no-import` to `build`/`update` to opt out (for example to keep the module
strictly hand-edited).

`build` and `update` run the same additive reconcile after a successful image
build, so a rebuild brings the live overlay in line with the module without
ever discarding what you installed.

`update` goes one step further and also **upgrades the packages already in the
overlay** (the backend's own upgrade verb — `kinpkg overlay-upgrade` /
`rum upgrade`),
so a system update refreshes the overlay too, not just installs the packages
the module declares. `build` does not do this. It prompts before applying, and `--force`
skips the prompt:

```bash
dcli-bootc overlay upgrade            # or: dcli overlay upgrade
dcli-bootc overlay upgrade --force    # skip the confirmation
```

Without `--force`, dcli-bootc passes `-y` to `kinpkg` (or `rum`) only *after*
you approve, so dnf does not prompt a second time. Running `kinpkg install`/
`remove` by hand behaves like normal `dnf`: it asks for confirmation unless you
pass `-y`/`--assumeyes`. (`kinpkg update`/`upgrade` is the *system-image*
update, not an overlay-only operation — see the KineticOS docs.)

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

These commands are no-ops on hosts without a usable overlay backend, and on
projects without `overlay_module`, so projects that don't use an overlay are
unaffected. If you do add the manifest module to `enabled_modules`,
`gen`/`build` warn: its packages would then be baked into the image as well as
installed in the overlay.

On kineticos the live-overlay reset step is skipped (`kinpkg` has no
`rakuos reset-overlay` equivalent): `clean` rewrites the recorded package
lists, and the change takes effect on the next overlay rebuild.

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
dcli-bootc build                    # Build and tag as the profile's default_tag
dcli-bootc build --nvidia           # NVIDIA base image variant, tagged :nvidia
dcli-bootc build --base-image deck  # Named variant from profile.yaml, tagged :deck
dcli-bootc build --no-cache         # Disable podman layer cache entirely
dcli-bootc build --reboot           # Reboot after switching
dcli-bootc build --no-switch        # Build only; never switch the deployment (CI)
```

Named variants default to the variant name as their tag instead of
`default_tag`, so the AMD/Intel and variant images coexist locally and can be
pushed as separate registry tags; `--tag` overrides either.

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

### Build storage

`build` and `update` default to `build_storage: optimized`. On the machine
running the build, dcli-bootc checks rootful Podman storage before generating
files, checking the registry, or pulling the base image. With the `overlay`
driver, it attempts to disable metacopy and enable native overlay diff to
avoid slow layer exports. If the storage already meets these requirements,
the build uses its existing settings.

When metacopy is active, dcli-bootc prepares `metacopy=off` for the build,
preserving the other driver options and all additional image stores. A second
`podman info` verifies native diff and unchanged storage identity before the
build starts. These global Podman options apply only to `podman build`;
other Podman commands and persistent storage configuration keep their settings.
If preparation or verification fails, dcli-bootc prints a prominent warning
with the cause and continues the build with the system Podman storage settings.
The build may be much slower, but it is attempted once; a failed `podman build`
is not retried. Unsupported or ambiguous driver options trigger this warning,
including option aliases whose ordering is lost in `podman info` and additional
layer stores for which it reports only the last value. Changed storage or
unavailable native diff does the same. Already optimized storage does not
require reconstructing its driver options.

To explicitly use the system Podman settings instead, set:

```yaml
# profile.yaml
build_storage: system  # Skip optimization checks; builds may be slow.
```

The default is `optimized` when omitted. A host file can override the profile
in either direction; omitting the host setting inherits the profile:

```yaml
# hosts/laptop.yaml
build_storage: optimized
```

This setting controls storage on the **build machine**, including when
building for another host. Other storage drivers keep their existing
behavior and report that the overlay diff check does not apply.

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
                             # (stays on the :nvidia tag)
dcli-bootc update --base-image deck  # Update the deck variant (stays on :deck)
dcli-bootc update --reboot   # Same, then reboot
dcli-bootc update --no-switch # Pull + rebuild but leave the deployment (CI)
```

`update` always pulls the latest base image, rebuilds on top of it, reconciles
the live usr-overlay to your manifest, and **upgrades the overlay's existing
packages** (after a confirmation; see
[Persistent usr-overlay](#persistent-usr-overlay-rakuos-rum--kineticos-kinpkg)).
`build` leaves the overlay packages as-is.

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

## CI/CD

Drop the included composite action and workflow templates into your own project to build and push bootc images in CI. They are meant to be **slotted into end-user repos**, not used to build `dcli-bootc` itself.

### GitHub Actions

Copy `.github/actions/dcli-bootc/action.yml` and `.github/workflows/build-image.yml` into your project. The workflow runs on `workflow_dispatch` — add a `schedule` trigger for daily builds:

```yaml
# .github/workflows/build-image.yml
on:
  workflow_dispatch:

  # Build the image daily
  schedule:
    - cron: "0 0 * * *"
```

The composite action installs `dcli-bootc`, runs your chosen subcommand, and optionally pushes the result to a registry:

```yaml
- name: Build and push bootc image
  uses: ./.github/actions/dcli-bootc
  with:
    project-path: "./my-bootc-image"
    command: "build"
    force: "true"
    registry: "ghcr.io/${{ github.repository_owner }}/my-image"
    registry-username: ${{ github.actor }}
    registry-password: ${{ secrets.GITHUB_TOKEN }}
```

Leave `registry` empty to skip the push step.

### GitLab CI

Include the template in your `.gitlab-ci.yml`:

```yaml
include:
  - project: "theblackdon/dcli-bootc/.gitlab/ci-templates"
    ref: main
    file: "/dcli-bootc.yml"
```

Then configure the variables:

```yaml
variables:
  BOOTC_PROJECT_PATH: "./my-image"
  BOOTC_REGISTRY: "registry.example.com/username/image"
```

The pipeline runs `gen` → `build` → (manual) `push`. Add a schedule in GitLab's UI to run it daily.

### Requirements

CI runners need:
- **podman** with root privileges and the `containers-storage` transport
- **bootc** (only needed if you want to switch/rollback from CI)
- **skopeo** for pushing to a registry
- **dnf** for installing packages during the build

Self-hosted runners or VMs with nested container support work. Shared runners without privileged containers will not.

## License

0BSD

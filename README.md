# dcli-bootc

A declarative **bootc** image configuration manager. Define your system
packages, repos, files, services, and scripts in YAML modules, then generate a
Containerfile and build the image.

## Commands

| Command | Description |
|---------|-------------|
| `init`  | Initialize a new bootc image project |
| `gen`   | Generate Containerfile and build script from YAML modules |
| `build` | Build the container image and optionally switch bootc deployment |
| `switch`| Switch the bootc deployment to the locally built image |
| `update`| Pull the latest base image, rebuild, and switch to the fresh build |
| `status`| Show bootc deployment status and project info |

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
├── files/              # Files to copy into the image
└── .dcli-bootc         # Project marker
```

## Configuration

`profile.yaml` defines the base image and enabled modules. Each module in
`modules/*.yaml` can declare packages, repos, remote RPMs, files, services,
and inline scripts.

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
so you can be sure the change actually landed. The new deployment only takes
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

## Installing from COPR

```bash
sudo dnf copr enable theblackdon/dcli-bootc
sudo dnf install dcli-bootc
```

On bootc hosts, prefer baking it into your image instead — host-installed
packages do not survive switching to a rebuilt image. Projects created with
`dcli-bootc init` do this automatically via the generated `base.yaml`.

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

## License

0BSD

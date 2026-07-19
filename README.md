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
| `update`| Update the base image tag and rebase |
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
dcli-bootc build --reboot    # Reboot after switching
```

## License

0BSD

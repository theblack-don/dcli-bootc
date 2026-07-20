Name:           dcli-bootc
Version:        0.1.0
Release:        1%{?dist}
Summary:        Declarative bootc image configuration manager

License:        0BSD
URL:            https://gitlab.com/theblackdon/dcli-bootc
# Sources are produced by `make srpm` (git archive + cargo vendor).
Source0:        %{name}-%{version}.tar.gz
Source1:        %{name}-%{version}-vendor.tar.gz
Source2:        cargo-config.toml

BuildRequires:  cargo
BuildRequires:  rust
# Runtime tools dcli-bootc shells out to
Requires:       bootc
Requires:       podman

%description
A declarative bootc image configuration manager. Define system packages,
repos, files, services, and scripts in YAML modules, then generate a
Containerfile, build the image, and switch the bootc deployment.

%prep
%autosetup -a1 -n %{name}-%{version}
mkdir -p .cargo
cp %{SOURCE2} .cargo/config.toml

%build
cargo build --release --offline

%install
install -Dm0755 target/release/%{name} %{buildroot}%{_bindir}/%{name}

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}

%changelog
* Sun Jul 19 2026 Black Don <theblackdonatello@gmail.com> - 0.1.0-1
- Initial package

Name:           dcli-bootc
Version:        0.6.0
Release:        1%{?dist}
Summary:        Declarative bootc image configuration manager

License:        0BSD
URL:            https://gitlab.com/theblackdon/dcli-bootc
# Produced by `make -f .copr/Makefile srpm` (git archive of HEAD).
Source0:        %{name}-%{version}.tar.gz

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
%autosetup -n %{name}-%{version}

%build
# Downloads crates from crates.io — the COPR project must have
# "Enable network during builds" turned on.
cargo build --release

%install
install -Dm0755 target/release/%{name} %{buildroot}%{_bindir}/%{name}
# Short alias so users can type `dcli build`, `dcli update`, ...
ln -s %{name} %{buildroot}%{_bindir}/dcli

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}
%{_bindir}/dcli

%changelog
* Wed Sep 30 2026 Black Don <theblackdonatello@gmail.com> - 0.6.0-1
- Version bump to 0.6.0

* Mon Sep 28 2026 Black Don <theblackdonatello@gmail.com> - 0.5.0-1
- Version bump to 0.5.0

* Sun Jul 19 2026 Black Don <theblackdonatello@gmail.com> - 0.1.0-1
- Initial package

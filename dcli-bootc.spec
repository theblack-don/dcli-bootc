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

# Shell completions, generated for both the full name and the short alias.
install -d %{buildroot}%{_datadir}/bash-completion/completions
install -d %{buildroot}%{_datadir}/fish/vendor_completions.d
install -d %{buildroot}%{_datadir}/zsh/site-functions
for bin in %{name} dcli; do
  target/release/%{name} completions bash --bin "$bin" > %{buildroot}%{_datadir}/bash-completion/completions/"$bin"
  target/release/%{name} completions fish --bin "$bin" > %{buildroot}%{_datadir}/fish/vendor_completions.d/"$bin".fish
  target/release/%{name} completions zsh  --bin "$bin" > %{buildroot}%{_datadir}/zsh/site-functions/_"$bin"
done

%files
%license LICENSE
%doc README.md
%{_bindir}/%{name}
%{_bindir}/dcli
%{_datadir}/bash-completion/completions/%{name}
%{_datadir}/bash-completion/completions/dcli
%{_datadir}/fish/vendor_completions.d/%{name}.fish
%{_datadir}/fish/vendor_completions.d/dcli.fish
%{_datadir}/zsh/site-functions/_%{name}
%{_datadir}/zsh/site-functions/_dcli

%changelog
* Wed Sep 30 2026 Black Don <theblackdonatello@gmail.com> - 0.6.0-1
- Version bump to 0.6.0

* Mon Sep 28 2026 Black Don <theblackdonatello@gmail.com> - 0.5.0-1
- Version bump to 0.5.0

* Sun Jul 19 2026 Black Don <theblackdonatello@gmail.com> - 0.1.0-1
- Initial package

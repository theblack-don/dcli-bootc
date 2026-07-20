# Convenience targets for local use; COPR invokes .copr/Makefile directly.

NAME    := dcli-bootc
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)

.PHONY: srpm rpm

srpm:
	@$(MAKE) -f .copr/Makefile srpm

# Build binary RPMs locally via the exact same pipeline COPR uses
# (dcli-bootc.spec + .copr/Makefile). --nodeps skips the BuildRequires
# check so this also works where cargo is not rpm-installed (e.g. rustup).
rpm: srpm
	@set -euo pipefail; \
	tmp=$$(mktemp -d); \
	rpmbuild --rebuild --nodeps --define "_topdir $$tmp/rpmbuild" \
		$(CURDIR)/dist/$(NAME)-$(VERSION)-*.src.rpm; \
	mkdir -p $(CURDIR)/dist; \
	find $$tmp/rpmbuild/RPMS -name '*.rpm' -exec cp {} $(CURDIR)/dist/ \;; \
	rm -rf $$tmp; \
	echo ">> Binary RPMs written to dist/"

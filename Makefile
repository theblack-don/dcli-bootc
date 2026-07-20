NAME    := dcli-bootc
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)

# COPR's "make srpm" build method passes `spec` and `outdir`;
# the defaults below cover local use (`make srpm`).
outdir  ?= $(CURDIR)/dist
spec    ?= $(NAME).spec

.PHONY: srpm
srpm:
	@set -euo pipefail; \
	tmp=$$(mktemp -d); \
	echo ">> Archiving $(NAME)-$(VERSION) sources from git HEAD"; \
	git archive --format=tar.gz --prefix=$(NAME)-$(VERSION)/ -o $$tmp/$(NAME)-$(VERSION).tar.gz HEAD; \
	echo ">> Vendoring Rust dependencies (offline RPM build)"; \
	cargo vendor --versioned-dirs $$tmp/vendor > /dev/null; \
	tar -C $$tmp -czf $$tmp/$(NAME)-$(VERSION)-vendor.tar.gz vendor; \
	printf '[source.crates-io]\nreplace-with = "vendored-sources"\n\n[source.vendored-sources]\ndirectory = "vendor"\n' > $$tmp/cargo-config.toml; \
	mkdir -p $(outdir) $$tmp/build; \
	rpmbuild -bs $(spec) \
		--define "_sourcedir $$tmp" \
		--define "_srcrpmdir $(outdir)" \
		--define "_builddir $$tmp/build"; \
	rm -rf $$tmp; \
	echo ">> SRPM written to $(outdir)"

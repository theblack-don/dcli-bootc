# COPR's "make srpm" SCM build method invokes this file as:
#   make -f .copr/Makefile srpm outdir=<dir> spec=<path-to-spec>
# The source-build chroot provides git/tar/rpm-build but no Rust toolchain,
# so this only archives the sources. Crates are fetched from crates.io during
# the binary build (requires network: enable it in the COPR project settings).
#
# Local use (from the repo root):  make srpm

NAME    := dcli-bootc
VERSION := $(shell sed -n 's/^version = "\(.*\)"/\1/p' Cargo.toml | head -n1)
outdir  ?= $(CURDIR)/dist
spec    ?= $(NAME).spec

.PHONY: srpm
srpm:
	@set -euo pipefail; \
	tmp=$$(mktemp -d); \
	git archive --worktree-attributes --format=tar.gz --prefix=$(NAME)-$(VERSION)/ -o $$tmp/$(NAME)-$(VERSION).tar.gz HEAD; \
	mkdir -p $(outdir) $$tmp/build; \
	rpmbuild -bs $(spec) \
		--define "_sourcedir $$tmp" \
		--define "_srcrpmdir $(outdir)" \
		--define "_builddir $$tmp/build"; \
	rm -rf $$tmp; \
	echo ">> SRPM written to $(outdir)"

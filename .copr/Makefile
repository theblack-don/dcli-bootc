# COPR's "make srpm" SCM build method invokes this file as:
#   make -f .copr/Makefile srpm outdir=<dir> spec=<path-to-spec>
# inside a minimal chroot that has tar/gzip/rpm-build but NO git binary,
# so the tarball is produced with plain tar over the checked-out tree.
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
	tar --exclude-vcs \
		--exclude='./target' --exclude='./dist' \
		--transform 's|^\.|$(NAME)-$(VERSION)|' \
		-czf $$tmp/$(NAME)-$(VERSION).tar.gz -C $(CURDIR) .; \
	mkdir -p $(outdir) $$tmp/build; \
	rpmbuild -bs $(spec) \
		--define "_sourcedir $$tmp" \
		--define "_srcrpmdir $(outdir)" \
		--define "_builddir $$tmp/build"; \
	rm -rf $$tmp; \
	echo ">> SRPM written to $(outdir)"

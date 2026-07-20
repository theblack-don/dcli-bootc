# Convenience wrapper for local use; COPR invokes .copr/Makefile directly.
.PHONY: srpm
srpm:
	@$(MAKE) -f .copr/Makefile srpm

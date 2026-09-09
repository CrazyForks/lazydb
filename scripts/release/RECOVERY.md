# v0.1.0 Updater Archive Compatibility

The shipped v0.1.0 Unix updater requires an explicit archive root directory,
but rejects paths with a trailing slash. Ordinary `tar` output and Python's
default `tarfile` directory serialization can therefore fail with an
`unsafe archive entry` error before the new binary is installed.

`package-archive.py STAGING OUTPUT.tar.xz` preserves the package root and its
contents while writing directory headers without trailing slashes. It uses
USTAR, fixes header checksums after serialization, sorts entries, and normalizes
timestamps, ownership, and modes. Symlinks and special files are rejected;
USTAR path/size limits apply. Python 3.9+ with lzma support is required.
Determinism is tested for identical contents and executable bits with the same
Python/liblzma toolchain; compressed bytes are not promised across toolchains.

Run `sh scripts/release/test-distribution.sh` for fixture coverage, or
`sh scripts/release/test-archive.sh /absolute/path/to/lazydb EXPECTED_VERSION`
to additionally verify an existing native binary. The regression compiles the
frozen v0.1.0 path validator with `rustc`, checks raw headers and checksums,
compares extracted contents, and runs `version --json`. The fixture target
label is not a cross-compilation check; use a binary runnable on the test host.
The release workflow also compares and smoke-tests each extracted Unix binary
on its native runner. Windows ZIP packaging is unchanged.

## Recovery Boundaries

This fix affects newly generated archives, not already published assets.
Prefer a normally approved new release built with this helper. Do not work
around the issue by dropping the directory entry: v0.1.0 requires it.

Replacing an existing release archive requires separate approval and coordinated
regeneration of SHA256SUMS, channel-manifest hashes, provenance, and any package
metadata that embeds archive hashes (including Homebrew). Preserve binary
contents and verify their reported version; changing just the archive still
changes its hash.

The existing manual release workflow checks out current `main` and can replace
assets, publish a release, and update Homebrew. It is not a packaging-only dry
run, and must not be dispatched merely to validate this fix. It also validates
the selected tag against the checked-out project version. No versions or
published assets are changed by the local regression tests.

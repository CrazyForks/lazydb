# Portable macOS Release Implementation Plan

> Execute task-by-task, recording actual verification results. Commit, push, tag, and publish only with explicit user authorization.

**Goal:** Remove the published macOS binary's Homebrew liblzma runtime dependency, reject non-portable release binaries, and diagnose installer Python LZMA failures before installation starts.

**Architecture:** Enable xz2's bundled static liblzma build without changing archive formats or updater APIs. Add an independent Mach-O dependency gate before execution and after packaging. Preserve existing installation roots and the previously implemented archive compatibility for shipped updaters.

**Tech Stack:** Rust, Cargo, xz2/lzma-sys, POSIX shell, Python 3, macOS otool, GitHub Actions.

---

## Evidence and Scope

- Published v0.1.1 aarch64-apple-darwin binary lists `/opt/homebrew/opt/xz/lib/liblzma.5.dylib` in `otool -L`.
- `Cargo.toml` currently declares `xz2 = "0.1"`. Upstream static feature disables pkg-config discovery and builds bundled liblzma.
- `scripts/release/check-binary-size.sh` prints dynamic dependencies but does not reject them.
- `pages/install-core.sh` uses Python tarfile and executes the staged binary's `version --json`. Python `_lzma` failure and dyld failure are separate problems.
- `install.sh` and `pages/install-core.sh` have different persisted installation roots. Do not merge their behavior or move user data as part of this fix.
- Keep `.tar.xz`, manifest schema, asset names, CLI, and the compatible TAR directory serialization unchanged.
- Do not install/uninstall Homebrew, modify system library paths, use DYLD workarounds, or overwrite released assets.
- A clean PATH is not proof of dynamic-library independence: dyld can load absolute Homebrew paths regardless of PATH.

## Task 1: Add a Failing Portability Gate

**Files:**
- Create `scripts/release/check-macos-dependencies.sh`.
- Create `scripts/release/test-macos-dependencies.sh`.
- Modify `scripts/release/test-distribution.sh`.

1. Write shell fixtures using a fake `otool` on a temporary PATH. No macOS SDK is required for parser tests.
2. Cover system-only output, ARM Homebrew, Intel Homebrew, MacPorts, build-directory paths, unresolved `@rpath`/`@loader_path` dependencies, missing binary, empty output, and nonzero otool exit.
3. Run `sh scripts/release/test-macos-dependencies.sh`; confirm the absent checker causes failure.
4. Implement checker with one binary argument. Check file existence; capture `otool -L` separately so failures cannot be hidden by a pipeline. Parse dependency records excluding the binary heading, removing only the compatibility/version annotation. Reject missing/malformed records rather than silently succeeding.
5. Allow only `/usr/lib/` and `/System/Library/` dependencies for this standalone executable. Report every rejected path and return nonzero. Do not use a narrow liblzma-only blacklist.
6. Add this regression to `test-distribution.sh`; run both scripts and require success.
7. Run checker against the downloaded published v0.1.1 ARM binary. Expected: failure explicitly naming the Homebrew dylib. This is a read-only check; do not execute or replace the old binary.

## Task 2: Make liblzma Linking Explicit

**Files:**
- Modify `Cargo.toml`.
- Inspect `Cargo.lock`; include a change only if Cargo actually requires it.

1. Replace the dependency with:

```toml
xz2 = { version = "0.1", features = ["static"] }
```

2. Run `cargo tree -e features -i lzma-sys`; confirm the static feature is enabled through xz2.
3. Build using `cargo build --release --locked` on macOS. Feature tracking should invalidate affected artifacts; do not delete unrelated worktrees/caches.
4. Run `sh scripts/release/check-macos-dependencies.sh target/release/lazydb` and `sh scripts/release/smoke-artifact.sh target/release/lazydb 0.1.1` while the project version is still 0.1.1. Use the actual project version if implementation occurs after a version bump.
5. Expected: no external liblzma dependency and matching version JSON. Check the actual binary, not just Cargo's feature output.
6. Run `cargo test --lib update::tests`; require TAR/ZIP, security, and release-packager regressions to remain green.

## Task 3: Add Installer Capability Preflight

**Files:**
- Modify `install.sh`.
- Modify `pages/install-core.sh`.
- Modify `scripts/release/test-installer.sh`.

1. Add a fake Python wrapper that fails only the new LZMA capability probe; delegate other invocations to the original Python interpreter.
2. Test the root installer, Pages stable entrypoint, and Pages beta entrypoint.
3. Assert missing capability produces a specific error before curl downloads, lock acquisition, installation-directory creation, or changes to existing `current`/`install.json`.
4. Implement the same probe immediately after `command -v python3` in both implementations:

```sh
python3 -c 'import lzma; lzma.LZMADecompressor(format=lzma.FORMAT_XZ)' >/dev/null 2>&1 \
    || die 'python3 cannot decode XZ archives; use a Python 3 installation with working lzma support. Homebrew is not required.'
```

5. The message must not claim every import failure means the module is absent: its own dynamic library could also be broken.
6. Add success coverage with fake `brew` and `xz` commands that fail if invoked. Existing archive construction can use host tools before entering the constrained installer environment; document this boundary.
7. Run `sh scripts/release/test-installer.sh` and `sh scripts/release/test-pages.sh`.
8. Do not add a tar fallback without equivalent pre-extraction member/type/path validation. Do not claim Python LZMA is no longer needed.

## Task 4: Wire Gates Into CI and Releases

**Files:**
- Modify `.github/workflows/ci.yml`.
- Modify `.github/workflows/release.yml`.
- Keep `scripts/release/check-binary-size.sh` as a size/dependency report; security enforcement is the explicit checker.

1. In regular macOS Rust CI, build the binary with `cargo build --locked`, then run the checker against `target/debug/lazydb`. This catches regressions before tagging without an extra release-LTO build on every push.
2. In release build jobs, run the checker against `target/${TARGET}/release/lazydb` on both `aarch64-apple-darwin` and `x86_64-apple-darwin`, before the smoke-test step.
3. After archive extraction, check the extracted macOS binary before executing it; retain byte comparison and version verification.
4. Keep Linux and Windows gates platform appropriate; do not accidentally invoke otool on ELF/PE binaries. Build all existing targets to verify static liblzma portability.
5. Retain the existing publish dependency on successful build jobs. A rejected dylib must prevent asset publication and subsequent channel promotion.
6. Fixture tests must demonstrate rejection even on a host where the forbidden library actually exists. Successful execution alone is insufficient.

## Task 5: Complete Local and Platform Verification

**Commands:**

```sh
cargo fmt --all -- --check
cargo clippy --all-targets --all-features -- -D warnings
cargo test --all-targets --all-features
sh scripts/release/test-distribution.sh
sh scripts/release/test-archive.sh target/release/lazydb 0.1.1
git diff --check
```

Use the current version for the real-binary archive test. Record any environmental failures separately from regressions.

**Acceptance matrix:**

| Case | Required outcome |
| --- | --- |
| Published broken ARM artifact | Dependency gate rejects Homebrew dylib |
| New ARM and Intel artifacts | System libraries/frameworks only |
| macOS without Homebrew | Extracted program starts and reports expected version |
| Python with working LZMA; no brew/xz commands | Official installer succeeds |
| Python without working LZMA | Specific error before download or installation mutation |
| TAR package generated by compatibility helper | Old path validator and new Rust extractor accept it |
| Old executable can start | CLI update to compatible new package succeeds |
| Old executable cannot start | External installer can install new version without executing old binary |
| Failed validation | Existing launcher, state, and user data remain unchanged |

For the no-Homebrew runtime test use a clean macOS VM/runner without external libraries, or a deliberately isolated test environment. Never rename/remove the user's Homebrew directory to simulate absence. If such an environment is unavailable, report dependency inspection as verified and clean-host execution as pending.

## Task 6: Recovery Documentation and Release Handoff

**Files:**
- Modify `scripts/release/RECOVERY.md`.
- Modify `README.md` installation/troubleshooting section if a public explanation is needed.
- Update `CHANGELOG.md` only during separately authorized release preparation using the project release skill.

1. Document dyld/liblzma versus Python/_lzma errors, with exact diagnostic distinctions.
2. Explain that users whose old executable starts can use CLI/TUI updates after channel promotion; users whose old executable fails to start need the external installer or manual replacement.
3. Recovery must preserve the existing installation root, launcher directory, and configuration. Discover those from the installation state/symlink rather than assuming root and Pages installers use the same directory.
4. Preserve the warning that installer `--version` currently must match the channel version; do not advertise arbitrary historical installation.
5. Prepare a new patch version (proposed v0.1.2), containing both directory-entry compatibility and static-linking fixes. Do not overwrite v0.1.1 assets or retag history.
6. Before public promotion, verify candidate archives, SHA-256, architecture, version JSON, and complete old-client upgrade behavior in an isolated installation. Old-client tests may need an isolated approved HTTPS fixture because manifests enforce allowed hosts; do not relax production URL validation or point real users to test manifests.
7. If the current release pipeline cannot stage this complete rehearsal before promotion, treat it as an explicit release-readiness gap. Do not claim the frozen path validator is a complete old-client upgrade test.
8. With separate release authorization, publish and verify the stable manifest's version, asset URLs, hashes, and fresh downloads. Follow the existing release skill, not manual ad hoc asset replacement.

## Completion and Authorization

- Implementation completion requires code, fixtures, local checks, and documented platform-verification results.
- Production readiness additionally requires actual ARM/Intel artifact gates and clean-host/old-client verification, or explicitly accepted remaining test gaps.
- Suggested commits, when authorized: `fix(release): statically link liblzma and reject external macOS libraries`; `fix(installer): check Python LZMA support before installation`.
- This plan does not authorize a version bump, commit, push, tag, published release, channel mutation, or user installation change.

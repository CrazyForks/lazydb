# Collapsed Release Commits Implementation Plan

> **For Claude:** REQUIRED SUB-SKILL: Use superpowers:executing-plans to implement this plan task-by-task.

**Goal:** Keep complete commit history in `CHANGELOG.md` while making the `Commits` section collapsed by default in GitHub Release notes.

**Architecture:** The release workflow already extracts a version section through `changelog-section.sh`. That script will transform only the extracted `### Commits` subsection into a GitHub-supported `<details>` block, leaving the tracked Changelog unchanged. The script test will verify both preservation of commit entries and the collapsed wrapper.

**Tech Stack:** POSIX shell, awk, Markdown, GitHub Releases.

---

### Task 1: Generate collapsed release notes

**Files:**
- Modify: `scripts/release/changelog-section.sh`
- Test: `scripts/release/test-changelog-tools.sh`
- Modify: `.opencode/skills/release/references/changelog-format.md`

1. Add a transformation after extracting the exact release section that wraps `### Commits` through the next `###`/`##` heading in `<details>`.
2. Keep `### Commits` as the summary heading inside the details block and preserve every commit line.
3. Extend the changelog tool test to assert the generated release section contains `<details>` and `</details>` around the commit list.
4. Document that tracked Changelog lists remain expanded while generated Release notes collapse them.
5. Run `sh scripts/release/test-changelog-tools.sh`.

### Task 2: Update the existing GitHub Release

**Files:**
- Generated: temporary release-notes file outside the repository

1. Generate release notes for `v0.1.5` with `scripts/release/changelog-section.sh 0.1.5`.
2. Update the existing GitHub Release body with `gh release edit v0.1.5 --notes-file ...`.
3. Verify the published body contains the collapsed wrapper and retains the commit entries.

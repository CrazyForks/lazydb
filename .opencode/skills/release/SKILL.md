---
name: release
description: Prepare LazyDB Beta or stable releases; use when asked to release, cut a version, publish a beta, update CHANGELOG.md, or create a release tag.
---

# LazyDB Release Skill

Use this skill only for an explicit LazyDB release request. The supported
commands are `release beta` and `release stable`.

This skill exists because a release is a coordinated, partly irreversible
operation: the version, changelog, source tree, commit, tag, remote branch,
and GitHub Actions publication must all describe the same release. It keeps
that sequence reproducible with one explicit version confirmation authorizing
the disclosed release scope, followed by automatic execution and verification.

## Safety

- Never revert, stash, force-push, overwrite an existing tag, or rewrite history.
- Stop if unrelated worktree changes are present.
- On a new release, require a clean worktree and index. Preparation may change
  only `CHANGELOG.md`, `Cargo.toml`, and `Cargo.lock`; do not bundle existing
  changes or fix application code, tests, or workflows to make a release pass.
- Fetch tags and inspect the actual diff before recommending a version.
- Do not expose credentials or put them in files.
- GitHub Actions must never create a version or modify the tagged source.

## Interaction Protocol

Treat the maintainer's short replies as state transitions, not as unrelated
questions:

1. `release beta` or `release stable` starts `INSPECT`, not immediate publication.
   An analysis-only request makes no changes. If the user says prepare only,
   stop after local preparation and validation without commit, tag, or push;
   if they permit commit/tag but prohibit pushing, stop before `PUSH`.
2. After inspection, present one recommended exact version, evidence, repository,
   branch, source HEAD, and scope. Explicitly state that confirmation authorizes
   changelog/version edits, checks, release commit, annotated tag, pushes to
   `origin/main` and `origin/vVERSION`, and waiting for CI, Release, and Pages,
   except for actions excluded by the user. Ask for `confirm`, `override VERSION`,
   or `stop`. State that a valid override selects that version for the same scope.
   Do not edit before this single version confirmation, even if the initial
   request already named a version.
3. Accept clear affirmative replies in the user's language; exact English
   tokens are not required. An unambiguous override in response to this prompt
   is confirmation once its format, channel, and local/remote tag uniqueness
   pass validation. Recompute version-dependent baselines and commit collection.
   An invalid override stays at version selection; never silently substitute one.
4. After confirmation, proceed automatically through every authorized stage.
   Diff summaries and command previews are progress reports, not approval gates.
   Do not ask for `confirm commit`, `confirm push`, or another routine approval.
   Do not treat a question, ambiguous reply, or narrower request as full consent.
5. On `stop`, leave all changes already made in place, report the exact state,
   and do not clean up by reverting, stashing, or resetting.

The normal path is `INSPECT -> AWAIT_VERSION_CONFIRMATION -> PREPARE -> VALIDATE
-> COMMIT_AND_TAG -> PUSH -> VERIFY_PUBLICATION -> COMPLETE`. Version confirmation
is the only routine human decision. A failed invariant enters `BLOCKED`, not a
new approval gate. A narrower requested scope ends when its authorized work is done.

This authorization does not bypass higher-priority instructions, OpenCode tool
permissions, branch protection, or GitHub Environment approval. Report such
restrictions as blockers; never change permissions or protection to avoid them.

## Procedure

1. Verify `main`, its upstream, a clean worktree and index, and the
   `yelog/lazydb` remote. Fetch the remote branch and tags without changing user
   files. Require local `main` to contain remote `main` and inspect any outgoing
   commits. Stop if behind or diverged; do not merge or rebase automatically.
   Check required tools and GitHub authentication before editing. Record source HEAD.
2. Determine the candidate line from actual tag history, then run
   `scripts/release/collect-commits.sh beta VERSION` or
   `scripts/release/collect-commits.sh stable VERSION`. Save its JSON output
   to a temporary file when it is large, and inspect both that data and
   `git diff BASE..HEAD`.
3. Recommend `MAJOR.MINOR.PATCH` using breaking changes, Conventional Commit
   evidence, affected code, and existing tags. For Beta use `VERSION-beta.1`,
   or increment the existing same-line Beta number. Validate the candidate
   before presenting it.
4. Wait for the version transition described above. A confirmed version is
   the single source of truth for all following commands.
5. Generate a dated Keep a Changelog body with Added, Changed, Fixed,
   Security, Deprecated, Removed, or Internal categories as appropriate. Add
   every collected commit exactly once to `### Commits`, including merge,
   revert, documentation, test, and CI commits. Use short SHA links to
   `https://github.com/yelog/lazydb`.
6. Write the body to a temporary file outside the repository and invoke
   `python3 scripts/release/update-changelog.py VERSION BODY_FILE DATE` so the
   section is inserted before `Unreleased`. Use `python3` explicitly because
   the script may not have its executable bit set. Update compare links if the
   repository maintains them.
7. Run `scripts/release/set-version.sh VERSION`, then validate the exact
   heading with `scripts/release/validate-changelog.sh VERSION` and
   `scripts/release/validate-version.sh --pre-tag vVERSION`.
8. Run `cargo fmt --all -- --check`, `cargo clippy --all-targets --all-features
   -- -D warnings`, `cargo test --all-targets --all-features`, and a release
   binary `version --json` smoke test. Also run
   `sh scripts/release/test-distribution.sh`; this is the shared CI/Release
   gate for installer, manifest, Pages, metadata, and online smoke-test contracts.
   All checks must pass before commit/tag creation. Report each command's actual result;
   never infer success from a partial or concurrent command.
9. Inspect the complete release diff, status, index, and recent commit history.
   Report the files changed, baseline, commit count, diff summary, and exact
   commands without pausing for approval. Verify HEAD still matches the inspected
   source and all changes are release-only. Stage only the three allowed files,
   inspect the staged diff, then create commit `chore(release): prepare vVERSION`
   and annotated tag `vVERSION` with message `LazyDB vVERSION`. Check local and
   remote tag absence immediately before creation. Respect any narrower scope.
10. Verify the commit contains only the validated release changes, tag target,
    clean worktree/index, remote identity, and remote tag absence. Refresh remote
    `main`; stop if it changed since inspection. Without another question, run
    `git push origin main` followed by `git push origin vVERSION`, stopping if
    either fails. Pushing the tag starts GitHub publication. Never force a push.
11. After pushing, verify local/remote refs and use `gh` to identify and wait
    for the CI and Release runs for the exact pushed commit/tag. Then identify
    the Pages run triggered by that Release run and wait for its completion,
    including every required online installation verification job.
    Do not accept an older successful run as evidence. Poll for the downstream
    run with a bounded timeout; if it never appears, report the blocker.
12. Report "release complete" only when CI, Release, and Pages (including
    online installation of the exact version) succeed. Include all run URLs.
    If `gh` is unavailable, a run fails, or monitoring times out, report
    "pushed; release verification incomplete" with the outstanding checks.
    If the user explicitly asks not to wait, use that same incomplete status.
    For an explicitly requested manual verification, run
    `sh scripts/release/smoke-online-install.sh CHANNEL VERSION`; it uses
    isolated temporary directories and never replaces the maintainer's install.
    Never rewrite tags or republish assets automatically to recover a failure.

## Changelog baselines

- First Beta: previous published tag, or repository root if no tag exists.
- Later Beta: previous Beta for the same base version.
- Stable: previous stable tag, intentionally including intervening Betas.

Do not silently omit merge or revert commits. If a commit is not user-facing,
put it in `Internal` or the traceable commit list.

## Automation Rules

- Prefer one structured inspection pass and parallel read-only checks where
  possible; serialize commands that share Cargo's package or build locks.
- Use the repository's release scripts as the source of truth instead of
  reproducing version or changelog logic in ad hoc commands.
- Keep temporary release-body and JSON files outside the repository and remove
  them after use.
- The single version confirmation authorizes all disclosed stages for that
  version and source scope. Never extend it to another release or unrelated work.
- Retry transient read-only network queries and polling with bounded timeouts.
  Before retrying a mutating command after a timeout, inspect actual local and
  remote state to determine whether it already succeeded.
- If a check or command fails, stop at `BLOCKED` and report the failed stage,
  actual error, completed mutations, and corrective action. Never skip checks,
  change application code, move tags, republish assets, or rerun failed publication
  jobs automatically. A normal stage boundary is not a reason to stop.

## Resume

Keep a concise state summary in the conversation: authorized scope, channel,
confirmed version, source HEAD, inspected remote-main SHA, release commit, tag,
push results, and workflow run IDs. Git and GitHub are authoritative; do not
introduce a tracked state file or rely on the summary without checking reality.
On resume, reconcile completed stages using these rules before continuing the
procedure. The fresh-run clean-tree and absent-tag requirements do not reject
verified changes, commits, or tags already created by this authorized run.

- Resume the same authorized release without repeating version confirmation
  when the authorization is available and the source and scope are unchanged.
  If authorization is unavailable, present the remaining scope with the version
  for confirmation; do not infer consent from an existing tag or commit alone.
- Verify existing preparation changes belong to this run and are release-only.
  Revalidate after any change; do not insert a duplicate Changelog section.
- If the release commit exists, verify its parent and exact diff. If the local
  tag exists, verify it is annotated and targets that commit. Skip completed
  operations only when they match this run; conflicting existing tags block.
- If the branch or tag is already pushed, verify remote refs before skipping
  that push. A remote branch equal to this run's release commit is expected on
  resume; any other change from the recorded remote-main SHA blocks further writes.
  A matching pushed tag means continue monitoring the existing publication.
- Unexpected source HEAD, version, channel, repository, or scope changes invalidate the
  old authorization. Stop and report the discrepancy rather than applying the
  old approval to new work. Never delete or reset completed work on recovery.

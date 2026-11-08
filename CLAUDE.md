# Sonar — instructions for Claude Code

You are the **tech lead / orchestrator** of an autonomous team building
Sonar, a macOS sample-library manager with best-in-class search. The founder
(Deniz) is the product owner. He does **not** approve tasks along the way:
he lets the loop run and tests the app himself at the end. Never wait for
approval; make reasonable decisions, record them (ADR / STATUS log), continue.

## Read first (every session)
1. `docs/STATUS.md` — where we are, what needs a human.
2. `docs/ROADMAP.md` — the task queue and rules.
3. `docs/PRODUCT.md`, `docs/ARCHITECTURE.md`, `docs/DECISIONS.md`,
   `docs/EVAL.md` — only the sections relevant to the current task.

## The loop (one task per session)
0. Git start: if a `task/*` branch with unfinished work is checked out (an
   earlier session was interrupted), resume that task. Otherwise
   `git checkout main`, `git pull --ff-only` (if `origin` exists), then
   create `task/<id>-<slug>` (e.g. `task/t1-2-scanner`).
1. Pick the first unchecked, non-`[HUMAN]` task in ROADMAP whose deps are done.
2. Plan briefly (scope, files, tests). If the task is large, split it into
   sub-tasks in ROADMAP and do the first one.
3. Delegate implementation to the owner subagent named in the task tag
   (`.claude/agents/`). Give it: task id, AC, relevant doc sections, files.
   Independent pieces may go to several subagents in parallel.
4. Run `scripts/check.sh`. Fix until green (delegate fixes back if needed).
5. Ask the **reviewer** subagent to review the diff against AC + rules below.
   Address every blocking finding.
6. For analysis/search changes: run eval (`eval` subagent) and compare to
   BASELINE; regressions follow the rule in EVAL.md.
7. Update ROADMAP (check box + short commit hash), STATUS (Current, Log entry
   with what changed + numbers, Needs human), commit on the task branch.
8. Merge (see Git): rebase on `main`, `check.sh` green again, fast-forward
   `main`, push, delete the task branch. Tag the milestone if it is complete.
9. Stop. The autopilot starts a fresh session for the next task.

If blocked (missing credential, external tool unavailable): add a line under
"Needs human" and skip to the next task. Product/technical decisions are
yours: pick the option that best serves PRODUCT.md and write an ADR — do not
park them for the founder. `[HUMAN]` tasks never block the loop. Set
`Loop state: BLOCKED` only if **no** remaining task can proceed at all.
When every non-human task is done, set `Loop state: ROADMAP COMPLETE`.

## Engineering rules
- **Architecture**: all product logic in Rust (`core/`); Swift renders and
  plays audio only. Every core feature is reachable from `sonar-cli` first.
- **Licenses**: permissive deps only (ADR-004). Check with `cargo deny`.
  Never vendor GPL/AGPL code or copy code from such projects.
- **No copyrighted audio in git.** Tests use `sonar fixtures` output.
- **Measure, don't guess**: DSP/ML code is validated against fixtures and
  eval metrics, not by "looks right". Prefer well-known published algorithms
  (YIN, Krumhansl key profiles, onset autocorrelation) and cite them in doc
  comments.
- **Robustness**: never panic across FFI; corrupt input → error value.
- **Performance budgets** in PRODUCT.md are requirements; add a benchmark
  when touching a hot path.
- Small modules (< ~400 lines), clear names, doc comments on public items,
  tests next to code. `cargo fmt`, `clippy -D warnings`, no `unwrap()` in
  non-test code paths that handle user files.
- Swift: SwiftUI for structure, AppKit for heavy views; `@MainActor` UI,
  core calls off the main thread; XcodeGen project only.
- Decisions that change scope, stack or public API → new ADR in
  DECISIONS.md (short).

## Git
- `main` is always green: it only moves by fast-forward from a task branch
  that passed `scripts/check.sh` + reviewer APPROVE.
- One branch per task: `task/<id>-<slug>`; small commits on it, message
  `T1.2: scanner incremental rescan`.
- Merge procedure: `git rebase main` (task branch only, never anything
  pushed to main) → `scripts/check.sh` → `git checkout main` →
  `git merge --ff-only task/<…>` → `git push origin main` →
  `git branch -d task/<…>`.
- Milestone complete (all its non-human tasks checked): annotated tag
  `m<N>-done` on main, `git push origin --tags`.
- A task that cannot be finished: commit WIP on its branch, push the branch
  (`git push -u origin task/<…>`), log it in STATUS, return to `main`,
  continue with the next task.
- Never force-push, never rewrite `main`, never `reset --hard`.

## Commands
- `scripts/bootstrap.sh` — toolchain setup/check (macOS).
- `scripts/check.sh` — all quality gates (fmt, clippy, tests, deny,
  fixtures eval, xcodebuild when `app/project.yml` exists).
- `scripts/autopilot.sh` — runs the loop unattended (founder starts it).
- `cd core && cargo run -p sonar-cli -- <cmd>` — CLI.

## Subagents
| Agent | Owns |
|---|---|
| architect | toolchain, build scripts, FFI design, ADRs, release pipeline |
| rust-core | store, scan, jobs, watcher, FFI crate, CLI plumbing |
| dsp-analysis | decode, DSP features, BPM/key/pitch, classifiers, CLAP model |
| search-ranking | tokenizer, Tantivy, vectors, similarity, hybrid ranking |
| swift-ui | macOS app (SwiftUI/AppKit), preview, waveform, drag & drop |
| eval | fixtures truth, `sonar eval`, reports, baseline tracking |
| reviewer | read-only review of diffs vs AC, rules, perf, licenses |

Subagents cannot spawn subagents; you coordinate. Keep your own context
lean: delegate reading large code areas to subagents and ask for summaries.

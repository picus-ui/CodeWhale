# v0.9.0 Execution Map

Snapshot date: 2026-06-04

This map tracks the v0.9.0 integration branch and keeps the open-PR harvest
separate from release publishing. It is a working document: update it whenever a
PR is harvested, superseded, deferred, or closed.

## Live Counts

- Actual open issues: 446
- Open PRs: 57
- Repo API open issue count: 503, because GitHub includes PRs in that total
- Open issues labeled `v0.9.0`: 119
- Open issues without a milestone: 102

## Execution Order

1. Stabilization and PR harvest: finish #2721 and #2722 before new feature work.
2. Provider/model/auth correctness: land narrow correctness fixes that match the
   current provider architecture.
3. HarmonyOS/MatePad Edge intake: keep #2634 credited while the local harvest
   clears the OHOS/Nix dependency chain; full target-build success still needs a
   host with the OpenHarmony native SDK loaded.
4. File decomposition Phase 1: split safe, test-covered config/provider and TUI
   view surfaces before adding larger workflow UX.
5. WhaleFlow MVP: typed IR, executor skeleton, replay, and pod monitor before
   teacher/student promotion loops.
6. Model Lab and HarnessProfile MVP: Hugging Face polish and provider/model
   posture before automatic harness creation.
7. Release readiness: keep #2729 current and do not tag or publish without
   maintainer approval.

## Current Branch Harvest

Branch: `codex/v0.9.0-stewardship`

The branch contains the previous 22-commit v0.9.0 stack plus these fresh
harvest/stewardship commits:

| PR | Disposition | Evidence / next step |
| --- | --- | --- |
| #2708 Windows sub-agent completion halves TUI render width | Cherry-picked as `e933a11d7`; follow-up fix `72653f8ef` invalidates reused fanout-card rows. | `cargo test -p codewhale-tui --locked subagent`; `cargo test -p codewhale-tui --locked terminal_size`; `cargo clippy -p codewhale-tui --locked -- -D warnings` passed. |
| #2627 Xiaomi MiMo Token Plan mode | Harvested only the auth-header behavior as `5aa68d986`; did not merge the conflicting mode/env changes. | `cargo test -p codewhale-tui --bin codewhale-tui --locked xiaomi_mimo`; `cargo test -p codewhale-secrets --locked xiaomi_mimo`; `cargo test -p codewhale-config --locked xiaomi_mimo`; `cargo clippy -p codewhale-tui --locked -- -D warnings` passed. |
| #2730 canonical codewhale settings path | Already harvested as `9e15805f6`; follow-up reviewer assertion added on this branch. | Fixes #2664 by reading legacy DeepSeek settings fallbacks, migrating them into `~/.codewhale/settings.toml`, and ensuring `/config` displays the canonical CodeWhale path. `cargo test -p codewhale-tui --bin codewhale-tui --locked settings_ -- --nocapture` passed. |
| Contributor credit plumbing | Added locally after the co-author audit. | Normalized unpushed harvest author/trailer emails to numeric GitHub noreply identities, added `.github/AUTHOR_MAP`, and wired `scripts/check-coauthor-trailers.py` into CI so future `Harvested from PR #N by @handle` commits require machine-readable credit. |
| #2640 workspace field on UpdateThreadRequest | Harvested with the stale-engine fix restored. | Added `workspace` to `PATCH /v1/threads/{id}`, rejects empty paths, rejects workspace changes during active turns, and evicts idle cached engines so the next turn uses the new workspace. `cargo test -p codewhale-tui --bin codewhale-tui --locked update_thread_workspace -- --nocapture` and `cargo clippy -p codewhale-tui --locked -- -D warnings` passed. |
| #2639 POST /v1/sessions endpoint | Locally harvested with the unsafe active-turn snapshot fixed. | Adds `POST /v1/sessions` so runtime clients can save a completed thread as a managed session, preserves title/model/mode/workspace metadata, maps missing threads to 404, and returns 409 while any turn or item is queued/in-progress. `cargo test -p codewhale-tui --bin codewhale-tui --locked session_create -- --nocapture` and `cargo test -p codewhale-tui --bin codewhale-tui --locked session_ -- --nocapture` passed. Credit @gaord; comment/close the original after the integration branch is public. |
| #2733 PlanArtifact for Plan mode | Locally harvested as a broader continuity-artifact slice. | Added rich `update_plan` fields for objective, context, sources, files, constraints, verification, risks, and handoff notes; renders them in the transcript card and Plan confirmation prompt; preserves them through `/relay`, fork-state, and saved-session replay. `cargo test -p codewhale-tui --bin codewhale-tui --locked plan_ -- --nocapture`, `cargo test -p codewhale-tui --bin codewhale-tui --locked relay_slash_command_routes_to_session_relay_instruction -- --nocapture`, and `cargo clippy -p codewhale-tui --locked -- -D warnings` passed. |
| #2736 sub-agent model inheritance | Locally harvested with explicit-override and provider-shaping tests. | Tool-agent routing now inherits the parent runtime model instead of hard-coding `deepseek-v4-flash`, while explicit DeepSeek-style tool-agent overrides still win. The `reasoning_effort = off` fast lane is covered by strict OpenAI-like provider request-shaping tests. Credit @h3c-hexin; comment/close the original after the integration branch is public. |
| #2737 configured `skills_dir` discovery | Locally harvested with explicit-config precedence. | The system prompt now unions workspace-discovered skills and configured `skills_dir` skills instead of treating the configured directory as a fallback. Explicit configured skills are inserted before global defaults so they are not lost behind a large global skill library. Credit @h3c-hexin; comment/close the original after the integration branch is public. |
| #2738 dense tool-call transcript collapse | Locally harvested with expansion, cache-key, and safety fixes. | Successful read/search/list-style tool runs collapse by default once they cross the density threshold; failures, running cells, shell/exec, patch/write/edit/delete, diff preview, plan update, and review cells stay visible. Users can expand a group with Enter/Space/mouse and can set `tool_collapse = "compact" | "expanded" | "calm"`. Credit @idling11 and issue #2692; comment/close the original after the integration branch is public. |
| #2740 dense tool-run collapse follow-up | Superseded by the local #2738 harvest. | The PR carries the same #2692 product direction but its reviewed head still depended on folded-thinking state before collapse could render and omitted MCP status/name handling. The local #2738 harvest already covers common-case collapse, MCP success/tool-name grouping, expansion/cell-map behavior, and `tool_collapse` modes with focused transcript-collapse tests. Credit @idling11; comment/close after the integration branch is public. |
| #2734 sidebar detail popovers | Locally harvested as the mouse-hover slice for #2694. | Work/Tasks/Agents hover metadata now stores row hitboxes, compact display text, and full source text so truncated checklist items, task/turn ids, and sub-agent ids/progress expand into a bordered wrapping popover. The harvest fixes reviewer risks from the PR by treating row metadata as authoritative, sizing by display width instead of bytes, and keeping source text untruncated. `cargo test -p codewhale-tui --bin codewhale-tui --locked sidebar_hover -- --nocapture`, `... work_hover_text_preserves_full_checklist_item ...`, and `... subagent_hover_text_preserves_full_agent_id_and_progress ...` passed. Credit @idling11; keep #2694 open for keyboard access, richer Work/Tasks/Agents metadata, redaction expansion, and clipping/snapshot coverage. |
| #2532 pending-input delivery-mode labels plus #2054 queued-edit recovery | Locally re-harvested and extended for #2054. | Pending-input preview rows label steer-pending, rejected-steer, queued-follow-up, and editing-queued-follow-up delivery modes. The accidental ↑ edit path is test-covered while loading, and `Esc` restores the original queued follow-up before cancelling the active turn. `cargo test -p codewhale-tui --bin codewhale-tui --locked pending_input_preview -- --nocapture`, `... queued_draft ...`, and `... accidental_queue_edit_while_loading_is_labeled_and_recoverable ...` passed. Credit @cyq1017; leave #2054 open only if row-level edit/drop/send controls are still required beyond the composer recovery fix. |
| #2029 sub-agent checkpoint continuation | Locally implemented as the live-timeout recovery slice. | Sub-agents now persist `SubAgentCheckpoint` metadata through state, results, projections, and transcript handles. The runner checkpoints local messages before API calls and after model/tool cycles; per-step API timeout marks the child interrupted with `continuable=true`; `agent_eval { continue: true }` resumes only live checkpointed interrupted children. Reload preserves checkpoint metadata, but cold-restart continuation is intentionally not claimed because the child task/input channel is not rehydrated yet. `cargo test -p codewhale-tui --bin codewhale-tui --locked subagent -- --nocapture`, `cargo fmt --all -- --check`, `git diff --check`, and `cargo clippy -p codewhale-tui --locked -- -D warnings` passed. Credit @qiyuanlicn for the recovery report; keep #2029 open only if cold-restart continuation or broader checkpoint UX remains required. |
| #1786 stale running task recovery | Locally implemented as the durable restart-safety slice. | `TaskManager::load_state` now marks tasks that were persisted as `running` in a prior process as failed with an explicit restart/interrupted error instead of requeueing them. Running tool-call summaries inside those stale tasks are also marked failed. `cargo test -p codewhale-tui --bin codewhale-tui --locked running_tasks_are_not_requeued_after_restart -- --nocapture` and `cargo test -p codewhale-tui --bin codewhale-tui --locked task_manager -- --nocapture` passed. Credit @bevis-wong; keep #1786 open for foreground shell hang root cause and careful LIVE-state watchdog work that does not abort legitimate foreground commands. |
| #697/#1827 bounded auto-generated project context | Locally implemented from the stabilization audit. | When no project instructions exist, startup now writes `.codewhale/instructions.md` from the bounded Project Context Pack data instead of an unbounded summary/tree scan. The generated file avoids the dynamic `<project_context_pack>` marker when that setting is disabled, keeps later top-level folders visible, and omits noisy directory tails. `cargo test -p codewhale-tui --bin codewhale-tui --locked auto_generated_context_is_bounded_for_many_file_workspace -- --nocapture` and `cargo test -p codewhale-tui --bin codewhale-tui --locked project_context_pack -- --nocapture` passed. Credit reporters @NASLXTO and @wuxixing, plus earlier context-cap/startup work from @linzhiqin2003 and @merchloubna70-dot; leave #697/#1827 open pending real massive-repo/manual startup verification. |
| #2636 project-context mtime cache | Defer direct merge; harvest only after cache key/signature is widened. | Must include constitution changes, auto-generated context deletion, canonical path equivalence, and overwrite detection before landing. |
| #2634 HarmonyOS port | Locally harvested with additional Nix-chain clearance; keep credited and do not close until the integration branch is public. | User-supplied MatePad Edge demo (`https://bilibili.com/video/av116689597368905`) confirms real-device interest. Added env-driven OpenHarmony SDK setup, OHOS platform guards/fallbacks, self-update disablement, and OHOS target gating for Starlark execpolicy parsing plus PTY support so published OHOS builds do not pull `nix` 0.28 through `rustyline` or `portable-pty`. `cargo check --workspace --all-features --locked`, focused PTY/clipboard tests, and `cargo tree --locked -p codewhale-tui --target aarch64-unknown-linux-ohos -i nix@0.28.0` passed; full OHOS target check is blocked on this host because `OHOS_NATIVE_SDK`/target CC/sysroot are not configured and `ring` cannot find `assert.h`. |
| #2687 append-only mode/approval prompt | Defer direct merge; draft has compile failures and Plan-mode prompt correctness risks. | Any future harvest must keep stable `message[0]` genuinely mode-agnostic, preserve mode/approval suffixes after capacity replans, and distinguish external overrides from persisted generated prompts. |
| #2581 provider fallback chain design doc | Manually harvested as `docs/rfcs/2574-provider-fallback-chain.md` because the current PR head has no net file changes. | Keep issue #2574 open for implementation; close/comment on #2581 after the integration branch is public, crediting @idling11 and reporter @hsdbeebou. |
| #2530 mention depth-cap hint | Already present in the current v0.9 stack as `a97675824` and `29f57665e`. | `cargo test -p codewhale-tui --locked try_autocomplete_file_mention_no_match` passed. |
| #2513 restore snapshot listing | Manually harvested as `bb39cf169` with explicit `/restore list 101` cap rejection. | `cargo test -p codewhale-tui --locked restore_`; `cargo fmt --all -- --check`; `cargo clippy -p codewhale-tui --locked -- -D warnings` passed. Keep #2494 open because this is only the restore-listing slice. |
| #2576 PrefixCacheChange first-freeze event | Already present in the current v0.9 stack through `29acb87a9d`. | `cargo test -p codewhale-tui --locked prefix_cache` passed. Do not close until this integration branch is public or merged. |
| #2502 web_run RwLock split | Manually harvested with panic-safe state write-back, `Arc<WebPage>` cache reads, and serialized cache tests. | `cargo test -p codewhale-tui --locked web_run`; `cargo clippy -p codewhale-tui --locked -- -D warnings`; `cargo fmt --all -- --check` passed. |
| #2517 turn_meta tail relocation | Manually harvested with the user-text content block first and volatile turn metadata last. | `cargo test -p codewhale-tui --locked turn_metadata`; `cargo test -p codewhale-tui --locked user_message_turn_meta_is_appended_not_prepended`; `cargo test -p codewhale-tui --locked post_edit_hook_injects_diagnostics_message_before_next_request`; `cargo test -p codewhale-tui --locked request_builder_keeps_tail_turn_meta_after_user_text_for_wire`; `cargo clippy -p codewhale-tui --locked -- -D warnings` passed. |

## Stabilization Gate Evidence (#2721)

This ledger is not closed yet. It records the evidence already attached to the
v0.9 branch so the remaining Windows/manual checks are explicit.

| Area | Current disposition | Evidence / remaining check |
| --- | --- | --- |
| Windows IME/input recovery (#1835) | Partially fixed, still release-blocking. | Current branch has Windows IME recovery and char-routing tests, but the issue remains open with Windows/WSL reports. Needs a real Windows Terminal IME smoke for focus loss, idle, mode switch, first keystroke, and Esc recovery. |
| Windows width/resize (#2708, #582 class) | Partially fixed on this branch. | #2708 is cherry-picked plus the fanout-card cache invalidation follow-up. `cargo test -p codewhale-tui --bin codewhale-tui --locked terminal_size -- --nocapture` passed. Still needs a real Windows Terminal resize smoke for #582 before #2721 closes. |
| Windows shell descendant hangs (#2498, #1812 class) | Partially fixed and already harvested. | Foreground orphan-pipe regression passed locally with `cargo test -p codewhale-tui --all-features --locked foreground_shell_does_not_block_on_orphaned_subprocess_pipe -- --nocapture`. PR #2498 should close as harvested, but #1812 remains open for broader input-poll freeze modes and Windows CI/manual confirmation. |
| Large-repo context startup (#697/#1827 class) | Partially covered. | Project-context pack ordering/budget/noise tests passed, and the auto-generated fallback now has a synthetic 1000-file startup smoke with `cargo test -p codewhale-tui --bin codewhale-tui --locked auto_generated_context_is_bounded_for_many_file_workspace -- --nocapture`. Still needs a real massive-repo/manual startup benchmark before closing #697 or #1827. |
| Sub-agent timeout and trust model (#1806, #719) | Fixed or covered in current branch. | `heartbeat_timeout_secs` clamp/default test passed, and `agent_open_description_explains_fresh_vs_forked_context_and_trust_model` asserts that sub-agent results are self-reports. |
| Sub-agent checkpoint/resume (#2029) | Partially covered. | Live per-step API timeout now preserves a continuable checkpoint and `agent_eval { continue: true }` resumes the parked child; `cargo test -p codewhale-tui --bin codewhale-tui --locked subagent -- --nocapture` passed with checkpoint/projection/persistence/continuation coverage. Cold-restart continuation is not implemented because persisted child tasks are not rehydrated; decide whether #2029 can close as live-timeout recovery or should remain open for restart-resume UX. |
| Live shell/session liveness (#1786) | Partially fixed, still release-blocking. | Durable task restart recovery now fails stale persisted `running` tasks instead of requeueing them, covered by `running_tasks_are_not_requeued_after_restart` and broader `task_manager` tests. Foreground shell hang root cause and LIVE-state watchdog recovery remain open; avoid aborting legitimate foreground `exec_shell` commands while adding stale-card recovery. |
| Queued/live input feedback (#2054) | Common accidental edit path covered. | Queued-message recovery/editing, pending-input delivery-mode labels, explicit editing-queued-follow-up preview state, and Esc restore semantics are covered by `queued_draft`, `pending_input_preview`, and `accidental_queue_edit_while_loading_is_labeled_and_recoverable` focused tests. Keep open only if v0.9 also requires row-level edit/drop/send controls rather than composer-level recovery. |
| Prompt/UI calmness (#1191) | Defer or narrow. | No release-blocking regression evidence yet; keep as polish unless a current user-facing prompt/UI failure is identified. |

## PR Harvest Queue

| PR | State | v0.9.0 disposition |
| --- | --- | --- |
| #1865 Pro Plan mode | Conflicting | Likely superseded by HarnessProfile/model-posture lane; review before closing. |
| #1893 TLS certificate verification toggle | Conflicting | Security-sensitive; review separately, not part of first v0.9 harvest. |
| #2045 NSIS installer and classroom checklist | Conflicting | Defer unless release-readiness needs Windows installer work. |
| #2048 live shell output | Mergeable but build-broken/stale | Defer; PR head fails `cargo check -p codewhale-tui --tests --locked`, matches jobs by command prefix, and misses newer `task_shell_start` / `task_shell_wait` cards. Harvest only via a task-id based rewrite. |
| #2113 independent scroll regions | Conflicting | Defer; likely overlaps current transcript/sidebar work. |
| #2239 i18n Phase 1-4b | Conflicting | Defer until localization lane. |
| #2242 typed persistent tool permission rules | Conflicting | Compare with #2721 stabilization and permissions model. |
| #2256 workspace crate consolidation | Conflicting | Do not merge during v0.9 stabilization. |
| #2269 approval details and shell previews | Conflicting | Review for small UI harvest only. |
| #2318 message_submit hook transform | Draft/conflicting | Defer; hook behavior must match lifecycle policy. |
| #2382 v0.8.48 release harvest | Draft/conflicting | Candidate to close as obsolete after confirming no unharvested commits. |
| #2476 fork migration parent links | Conflicting / already harvested | Patch-equivalent work is already present on `origin/main` and this branch as `b76a11b99` plus follow-up `18550339a`. Close/comment original after the integration branch is public, crediting @cyq1017; close issue #2082 only after confirming the remaining `message_type` wording is obsolete. |
| #2479 ProviderKind/ApiProvider trait collapse | Conflicting | Defer until file decomposition Phase 1 reduces config surface. |
| #2482 WhaleFlow orchestration | Draft/conflicting | Inspect for IR ideas; do not merge wholesale. |
| #2486 WhaleFlow cost tracking | Draft/conflicting | Inspect after #2482; harvest telemetry ideas only. |
| #2491 typed ask permissions schema | Conflicting | Prior memory says safe candidate; verify current permissions work first. |
| #2498 Windows shell process trees | Conflicting / already harvested | Patch-equivalent work is already present on `origin/main` and this branch through the Windows JobObject cleanup commits. Close/comment PR #2498 as harvested, crediting @aboimpinto; leave issue #1812 open because this fixes descendant pipe-handle hangs but not every reported Windows input-poll freeze mode. |
| #2501 in-process LLM response cache | Conflicting | Defer; cache key risks noted in prior review. |
| #2502 web_run RwLock split | Mergeable | Manually harvested with panic-safety and shared cached-page reads; close/comment after branch is public. |
| #2505 subagent cap accounting | Draft/conflicting | Compare with current subagent cap tests before harvest. |
| #2506 provider path suffix overrides | Draft/conflicting | Partly superseded by current provider path-suffix support; verify. |
| #2507 stream chunk timeout config | Draft/conflicting | Defer unless stabilization needs it. |
| #2508 configurable path suffix | Conflicting | Likely superseded by #2506/current code; verify linked issue #2089. |
| #2509 parallel read-only web search | Closed / already merged via #2504 | Already present in `origin/main` as `a09af2024`; closed as harvested/superseded on 2026-06-04. |
| #2510 custom DuckDuckGo endpoint | Draft/mergeable | Low priority; defer unless docs/search lane takes it. |
| #2511 ToolCallBefore hooks | Conflicting | Defer to hook lifecycle lane. |
| #2512 custom completion sounds | Draft/conflicting | Defer. |
| #2513 restore snapshot listing | Draft/mergeable | Manually harvested as `bb39cf169` with cap-rejection polish; close/comment after branch is public, leave #2494 open. |
| #2517 turn_meta tail relocation | Mergeable | Manually harvested on the v0.9 branch; close/comment after branch is public. |
| #2520 prompt base disk cache | Mergeable | Defer. Review found unused prompt-cache infrastructure with no runtime wiring, cache keys that still require building the prompt first, real-home cache writes in tests, and a contract that depends on the deferred #2687 prompt split. |
| #2522 hard compaction preserving system segment | Mergeable | Defer. Review found a dormant hard path that would duplicate/cache summaries into the mutable system prompt if wired through current engine flow, and a simple tail split that can break tool-call pair and pinning invariants. |
| #2526 shell tool availability docs | Draft/conflicting | Likely superseded by tool-surface docs; verify before closing. |
| #2528 background completion wait | Draft/conflicting | Defer unless failing tests prove need. |
| #2529 workspace shell opt-in | Draft/conflicting | Review with permissions/sandbox stabilization. |
| #2530 mention depth cap hint | Draft/mergeable | Already present locally as `a97675824` and `29f57665e`; close/comment after branch is public. |
| #2576 PrefixCacheChange events | Mergeable | Already present locally through `29acb87a9d`; close/comment after branch is public or merged. |
| #2578 turn_end observer hook | Conflicting | Defer to hook lifecycle lane. |
| #2579 AppendLog session messages | Conflicting | Defer; large architectural change. |
| #2581 provider fallback chain design doc | Mergeable / empty diff | Manually harvested into `docs/rfcs/2574-provider-fallback-chain.md`; close original PR after branch is public, keep #2574 open for implementation. |
| #2623 plan prompt modal scroll support | Mergeable | Already harvested into the 22-commit stack. Comment/close original after integration branch is public. |
| #2627 Xiaomi MiMo Token Plan mode | Conflicting | Partially harvested; leave original open or comment with remaining mode/env scope once branch is public. |
| #2631 estimated_input_tokens cache | Mergeable | Already harvested into the 22-commit stack. |
| #2632 tool-catalog JSON cache | Mergeable | Already harvested into the 22-commit stack. |
| #2633 capacity reverse scans | Mergeable | Already harvested into the 22-commit stack. |
| #2634 HarmonyOS port | Draft / locally harvested | Harvested with credit and extra Nix-chain fixes. Keep the original PR open for now; comment after the integration branch is public and request a real OHOS SDK build confirmation from the contributor before closing. |
| #2635 output rows cache | Mergeable | Already harvested into the 22-commit stack. |
| #2636 project-context cache | Conflicting | Defer/harvest only after cache correctness fixes. |
| #2639 POST /v1/sessions endpoint | Mergeable / locally harvested | Harvested with a 409 guard for queued/in-progress turns/items, 404 missing-thread mapping, saved-session metadata preservation, and focused session endpoint tests. Comment/close after the integration branch is public, crediting @gaord. |
| #2640 workspace field on UpdateThreadRequest | Mergeable | Harvested locally with extra tests and engine-cache invalidation. Comment/close original after integration branch is public, crediting @gaord. |
| #2646 release publish hardening | Mergeable | Already harvested into the 22-commit stack. |
| #2687 append-only mode/approval prompt | Draft/mergeable | Defer. Review found compile failures and Agent-mode prompt leakage into Plan sessions via hard-coded prompt refresh. |
| #2708 Windows width fix | Mergeable | Cherry-picked and patched locally. |
| #2730 canonical codewhale settings path | Mergeable | Already harvested as `9e15805f6`; follow-up reviewer assertion added locally. Comment/close original after integration branch is public, crediting @xyuai and issue #2664. |
| #2732 pausable command lifecycle | Draft/mergeable | Defer; review flagged behavior changes. |
| #2733 PlanArtifact UI | Mergeable | Locally harvested with richer schema, rendering, relay/fork-state propagation, and replay tests. Comment/close original after integration branch is public, crediting @idling11 and issue #2691; keep #2691 open only if additional PlanReview product work remains. |
| #2734 sidebar detail popovers | Mergeable / locally harvested | Harvested the mouse-hover popover slice with row-source fixes and tests. Comment on the original after the integration branch is public, crediting @idling11; leave #2694 open for keyboard navigation and richer structured detail acceptance criteria. |
| #2736 sub-agent model inheritance | Mergeable | Locally harvested with parent-model inheritance, explicit override coverage, and strict OpenAI-like `reasoning_effort = off` shaping coverage. Comment/close original after the integration branch is public, crediting @h3c-hexin. |
| #2737 configured `skills_dir` discovery | Mergeable | Locally harvested with extra configured-before-global precedence tests. Comment/close original after the integration branch is public, crediting @h3c-hexin. |
| #2738 dense tool-call transcript collapse | Mergeable / locally harvested | Harvested with normal rendering preserved, expansion wired through Enter/Space/mouse, compact default restored, full-detail index mapping preserved for Alt+V/copy-style paths, and revision keys mixed across hidden cells. Comment/close original after the integration branch is public, crediting @idling11 and issue #2692. |
| #2740 dense tool-run collapse follow-up | Mergeable / superseded locally | Same #2692 lane as #2738. Reviewed PR head still had the common-case collapse and MCP grouping/name issues; local #2738 harvest already fixed those and added the focused tests. Comment/close after the integration branch is public, crediting @idling11. |

## Issue Reduction Strategy

Issue count should drop through evidence-backed consolidation, not bulk closing.

- Close fixed issues only after the v0.9 integration branch is pushed or merged
  and the relevant tests/checks are named in the closure comment.
- Close obsolete release-harvest PRs/issues after verifying no unique commits or
  linked reports remain.
- Supersede older OPENCODE, memory, web, VS Code, and cache-maximalism tickets
  into the current v0.9 lanes when their acceptance criteria are now covered by
  #2667, #2720-#2729, or a narrower current issue.
- Remove or defer `v0.9.0` scope from valid but non-release-critical roadmap
  issues instead of closing them.
- Always credit PR authors, issue reporters, and useful reviewers when a
  contributor branch is harvested.

## Immediate Next Actions

1. Prepare public comments for #2476, #2498, #2708, #2502, #2513, #2530,
   #2576, #2581, #2627, #2634, #2636, #2639, #2687, #2736, #2737, #2738, and
   already-harvested performance PRs.
2. Start file decomposition Phase 1 only after the PR harvest table has no
   unknown high-priority provider/prompt/cache branches.

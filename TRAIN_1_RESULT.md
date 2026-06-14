# Train 1 Result - Route/model isolation

Branch: `codex/v0.8.61-train-1`

No PR opened. No push/tag/publish/release performed.

## Commits

- `979a8a669` `fix(tui): route auto model dispatch through inventory` (`Refs: #3205`)
- `8af3ab4d3` `fix(engine): derive route budgets from context service` (`Refs: #3204`)
- `d14bf7699` `fix(tui): hide mode labels from runtime prompt` (`Refs: #3213`)
- `9f1094d31` `feat(tui): hydrate model facts from offline catalog` (`Refs: #3072`)
- `975496e6a` `fix(tui): derive model picker hints from registry` (`Refs: #3073`)
- `65f60d2e5` `fix(tui): expose cross-provider catalog rows in model picker` (`Refs: #3075`)
- `4d70d7c53` `fix(tui): expire terminal sub-agent cards` (`Refs: #3025`)
- `3761339bb` `fix(tui): route subagents by worker profile` (`Refs: #2027`, `Refs: #1768`)

## Issue Results

### #3205 - Route-effective model inventory/service

Status: implemented.

Files:
- `crates/tui/src/core/engine.rs`
- `crates/tui/src/core/ops.rs`
- `crates/tui/src/main.rs`
- `crates/tui/src/model_routing.rs`
- `crates/tui/src/runtime_threads.rs`
- `crates/tui/src/tui/auto_router.rs`
- `crates/tui/src/tui/ui.rs`

What landed: auto route dispatch resolves through the route-effective inventory/provider path, carries the resolved provider/model through engine/runtime/UI/headless surfaces, and activates provider-aware clients instead of treating model ids as globally valid.

Tests:
- `cargo test -p codewhale-tui model_routing` - pass, 22 tests passed.

Risks/follow-up:
- Broader provider-readiness UX is still conservative; this slice focuses on effective dispatch and validation rather than adding new warning surfaces.

### #3204 - Context-window metadata and over-limit preflight

Status: implemented.

Files:
- `crates/tui/src/core/engine.rs`
- `crates/tui/src/core/engine/context.rs`
- `crates/tui/src/core/engine/tests.rs`

What landed: engine context preflight now derives route/model budget metadata through `ContextBudget` and rejects/warns before issuing an over-window request.

Tests:
- `cargo test -p codewhale-tui context_budget` - pass, 22 tests passed.

Risks/follow-up:
- Unknown/custom model ids still depend on fallback metadata. Better live provider introspection can tighten those cases later.

### #3213 - Split model capabilities from human mode labels

Status: implemented.

Files:
- `crates/tui/src/core/engine.rs`
- `crates/tui/src/core/engine/tests.rs`
- `crates/tui/src/prompts.rs`

What landed: runtime prompts no longer expose human-facing mode/approval labels as the model-facing contract. The model now receives capability/profile/review-gate language instead.

Tests:
- `cargo test -p codewhale-tui runtime_prompt` - pass, 3 tests passed.
- `cargo test -p codewhale-tui runtime_policy_reference` - pass, 1 test passed.

Risks/follow-up:
- This covers the runtime prompt surface touched by this train; unrelated docs/help copy can still be audited separately.

### #3072 - Hydrate model registry from an offline cache

Status: partial.

Files:
- `crates/tui/assets/model_catalog.bundled.json`
- `crates/tui/src/main.rs`
- `crates/tui/src/model_catalog.rs`
- `crates/tui/src/models.rs`
- `crates/tui/src/pricing.rs`
- `crates/tui/tests/integration_mock_llm.rs`
- `crates/tui/tests/reasoning_content_replayed_after_tool_call.rs`

What landed: bundled offline catalog parsing, disk cache load/store, OpenRouter response normalization, merge precedence (`user override > provider cache > bundled`), and catalog-backed context/pricing metadata.

Tests:
- `cargo test -p codewhale-tui model_catalog` - pass.
- `cargo test -p codewhale-tui model_metadata` - pass.
- `cargo test -p codewhale-tui pricing` - pass, 22 tests passed.

Risks/follow-up:
- Live provider refresh, cache provenance UI, and user-facing cache management remain follow-up work.

### #3073 - Migrate hard-coded model lists to `model_registry`

Status: partial.

Files:
- `crates/tui/src/tui/model_picker.rs`
- `crates/tui/src/model_registry.rs`
- `crates/tui/src/model_catalog.rs`
- `crates/tui/src/models.rs`
- `crates/tui/src/pricing.rs`

What landed: `/model` picker hints now derive metadata from `model_registry` plus catalog/pricing data instead of a local hard-coded hint match; seeded cross-provider rows also flow through registry provider mapping.

Tests:
- `cargo test -p codewhale-tui model_picker` - pass, 35 tests passed.
- `cargo test -p codewhale-tui model_registry` - pass, 9 tests passed.
- `cargo test -p codewhale-tui pricing` - pass, 22 tests passed.

Risks/follow-up:
- Some provider defaults and compatibility lists remain in config/client layers. This train moved the model-picker-facing list and metadata path, not every legacy table in the workspace.

### #3075 - Provider/model selection correctness follow-on

Status: partial.

Files:
- `crates/tui/src/model_registry.rs`
- `crates/tui/src/tui/model_picker.rs`

What landed: registry provider families map to concrete serving providers, and `/model` can surface/select cross-provider catalog rows with the correct provider attached.

Tests:
- `cargo test -p codewhale-tui model_registry` - pass, 9 tests passed.
- `cargo test -p codewhale-tui model_picker` - pass, 35 tests passed.

Risks/follow-up:
- Provider-scoped search/filtering and richer provenance remain follow-up work.

### #3025 - Provider/model selection correctness follow-on

Status: partial.

Files:
- `crates/tui/src/tui/app.rs`
- `crates/tui/src/tui/subagent_routing.rs`
- `crates/tui/src/tui/ui.rs`
- `crates/tui/src/tui/ui/tests.rs`

What landed: terminal sub-agent activity cards now receive terminal timestamps, expire after five minutes, and cap retained terminal rows while preserving running cards.

Tests:
- `cargo test -p codewhale-tui reconcile_subagent_activity_state` - pass, 3 tests passed.

Risks/follow-up:
- Pin/manual-clear behavior and separate retention policy for failed cards remain follow-up work.

### #2027 - Per-role scout-vs-synthesis routes

Status: implemented for spawn-time route resolution.

Files:
- `crates/tui/src/tools/subagent/mod.rs`
- `crates/tui/src/tools/subagent/tests.rs`

What landed: `agent_open` assignment routing now resolves through `WorkerRuntimeProfile::for_role(...).model`. Explore/tool roles default to `ModelRoute::Auto` and take the provider cheap lane when available; synthesis roles default to `ModelRoute::Inherit`. Providers without a cheap tier keep the parent model rather than fabricating a DeepSeek id.

Tests:
- `cargo test -p codewhale-tui route_resolution_matrix_uses_worker_profile_model_routes` - pass, 1 test passed.
- `cargo test -p codewhale-tui tool_agent_route` - pass, 3 tests passed.
- `cargo test -p codewhale-tui worker_profile` - pass, 8 tests passed.
- `cargo test -p codewhale-tui auto_route_on_provider_without_cheap_tier_stays_on_parent_model` - pass, 1 test passed.

Risks/follow-up:
- Fleet-wide enforcement of worker profiles remains part of the broader worker-profile follow-up. This train wires the sub-agent spawn route.

### #1768 - Per-role/model override route correctness

Status: implemented for existing sub-agent role/model overrides; partial for broader UX.

Files:
- `crates/tui/src/tools/subagent/mod.rs`
- `crates/tui/src/tools/subagent/tests.rs`

What landed: explicit configured sub-agent models resolve as `ModelRoute::Fixed`, inherited routes preserve session-local provider/model state, and the resolved route carries `RequestTuning` metadata for reasoning effort plus the sub-agent response token ceiling.

Tests:
- `cargo test -p codewhale-tui route_resolution_matrix_uses_worker_profile_model_routes` - pass, 1 test passed.
- `cargo test -p codewhale-tui tool_agent_route` - pass, 3 tests passed.
- `cargo test -p codewhale-tui request_tuning` - pass, 10 tests passed.
- `cargo test -p codewhale-tui subagent_auto_model` - pass, 1 test passed.

Risks/follow-up:
- No new user-facing per-role route configuration UI was added; this uses the existing role/type model override path plus worker profile defaults.

## Final Verification

- `cargo fmt` - pass.
- `git diff --check` - pass.
- `git status --short --branch` before the final implementation commit showed only worktree-local paths under `crates/tui/src/tools/subagent/`.

Note: one attempted command, `cargo test -p codewhale-tui worker_profile request_tuning`, failed because Cargo accepts only one test filter before `--`. It was rerun as separate `worker_profile` and `request_tuning` commands, both passing.


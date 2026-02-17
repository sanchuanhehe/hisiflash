<!--
Sync Impact Report
- Version change: 1.0.0 → 1.1.0
- Modified principles:
	- II. Testing Defines Compatibility → II. Testing & Baseline-Driven Compatibility
- Added sections:
	- None
- Removed sections:
	- None
- Templates requiring updates:
	- ✅ .specify/templates/plan-template.md
	- ✅ .specify/templates/spec-template.md
	- ✅ .specify/templates/tasks-template.md
	- ⚠ pending (N/A path not present): .specify/templates/commands/*.md
- Runtime guidance docs reviewed:
	- ✅ README.md (no conflicting constitution references found)
	- ✅ CONTRIBUTING.md (no conflicting constitution references found)
	- ✅ AGENTS.md (no conflicting constitution references found)
- Follow-up TODOs:
	- None
-->

# hisiflash Constitution

## Core Principles

### I. Code Quality Is Non-Negotiable
All production code MUST pass `cargo fmt --all`, `cargo clippy --all-targets --all-features -- -D warnings`, and `cargo test` before merge. Public APIs MUST include documentation, and reviewers MUST reject changes that increase incidental complexity without clear benefit. Quick fixes that bypass linting, tests, or review quality are prohibited.

Rationale: hisiflash is a hardware-facing tool where small defects can cause firmware flashing failures. Strict quality gates reduce regression risk.

### II. Testing & Baseline-Driven Compatibility
Behavioral compatibility is defined by tests, not by assumption. Any user-visible change to flags, defaults, exit codes, JSON output, stdout/stderr routing, or i18n text structure MUST include updated or new tests in `hisiflash-cli/tests/` and relevant unit tests. Bug fixes MUST include a regression test that fails before the fix and passes after.

Before a new feature spec is finalized, authors MUST review current code and existing tests to identify the real baseline behavior. The spec MUST explicitly document what behavior is retained, changed, or deprecated relative to that baseline.

Rationale: the CLI is consumed by both humans and automation; compatibility must remain verifiable and explicit.

### III. UX & CLI Contract Consistency
CLI user experience MUST remain consistent across commands. Help/version behavior, option naming patterns, interactive vs non-interactive semantics, and localization behavior MUST be predictable and documented. Machine-readable modes MUST emit parseable JSON and MUST avoid stderr contamination when JSON mode is requested.

Rationale: inconsistent CLI behavior breaks scripts, increases support burden, and degrades operator trust.

### IV. Performance Budgets Are Required
Changes on critical paths (serial I/O, flashing pipeline, FWPKG parsing, monitor output) MUST preserve or improve baseline performance and responsiveness. Feature plans MUST define measurable performance targets and constraints (for example p95 latency, throughput, memory budget, or startup time). Any intentional performance regression MUST be justified and approved in review.

Rationale: flashing reliability and developer feedback loops depend on predictable performance.

### V. Simplicity and Maintainability
Designs MUST prefer the simplest solution that satisfies requirements. New abstraction layers, dependencies, and architectural expansion MUST include explicit justification in the implementation plan. YAGNI applies: speculative extensibility without active need is disallowed.

Rationale: maintainable systems evolve faster and safer than over-engineered systems.

## Quality Gates and Release Criteria

- Pull requests MUST satisfy: formatting, clippy (`-D warnings`), and all relevant tests.
- CLI contract changes MUST be reflected in `docs/testing/CLI_COMPATIBILITY_MATRIX.md`.
- Release tags MUST follow repository policy (`cli-v*` and `lib-v*`) and include changelog updates.
- Hardware-affecting changes MUST include manual validation steps in release or testing checklists.

## Development Workflow and Review Standards

- Feature work MUST flow through `spec -> plan -> tasks` with constitution checks in planning.
- Spec drafting MUST include an explicit baseline review against existing code paths and test contracts.
- Reviews MUST verify compatibility impact on: flags, exit codes, stdout/stderr, JSON contracts, and i18n behavior.
- Performance-sensitive work MUST include before/after measurement notes or clear justification if measurement is not feasible.
- Merges that violate P0 compatibility guarantees are prohibited unless accompanied by an explicit breaking-change plan.

## Governance

This constitution supersedes conflicting local practices for specification, planning, implementation, and review.

Amendment process:
1. Propose changes in `.specify/memory/constitution.md` with rationale and impact.
2. Update dependent templates and compatibility documentation in the same change.
3. Obtain maintainer approval before merge.

Versioning policy:
- MAJOR: principle removals/redefinitions or governance changes that alter mandatory behavior.
- MINOR: new principle/section or materially expanded mandatory guidance.
- PATCH: clarifications, wording improvements, and non-semantic edits.

Compliance review expectations:
- Every implementation plan MUST pass constitution gates before design/implementation.
- Every pull request review MUST check constitutional compliance and test evidence.
- Exceptions MUST be explicit, time-bound, and documented in the related plan/PR.

**Version**: 1.1.0 | **Ratified**: 2026-02-18 | **Last Amended**: 2026-02-18


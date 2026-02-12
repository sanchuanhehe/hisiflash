# CLI Review Rubric

Use this rubric to review CLI changes quickly and consistently in PRs.

## How to use

1. Score each of the 8 dimensions (0–3).
2. Mark blocker conditions.
3. Decide: `Approve` / `Request Changes` / `Needs Discussion`.
4. Paste the “Review Summary Template” into the PR comment.

---

## Scoring rubric (0–3 per dimension)

### 1) Argument syntax and predictability
- **3**: Syntax is clear, consistently options-first, supports `--`, no ambiguity.
- **2**: Mostly usable; minor inconsistencies do not affect primary workflows.
- **1**: Noticeable ambiguity or implicit rules; users must trial-and-error.
- **0**: Confusing syntax; common invocations are unpredictable/unstable.

### 2) Option naming consistency
- **3**: Long/short options are standardized; same-name means same-meaning across subcommands.
- **2**: A few naming issues, but understandable and documented.
- **1**: Same-name/different-meaning or abbreviation conflicts.
- **0**: No naming strategy; frequent collisions.

### 3) Help / Version / Discoverability
- **3**: Supports `-h/--help` and `--version`; help includes examples and next-step guidance.
- **2**: Core help exists, but examples/navigation are limited.
- **1**: Help misses key options or behavior details.
- **0**: Missing standard help behavior or outputs incorrect help.

### 4) Output contract (stdout/stderr + machine mode)
- **3**: stdout is data-only, stderr is diagnostics-only; stable machine mode exists (e.g. `--json`).
- **2**: Mostly separated, with minor edge-case mixing.
- **1**: Logs/messages pollute stdout and weaken script integration.
- **0**: No output contract; automation is difficult.

### 5) Error handling and exit codes
- **3**: Errors are actionable (cause + fix), and exit code semantics are stable and documented.
- **2**: Errors are mostly diagnosable; exit-code coverage is partial.
- **1**: Vague errors or inconsistent exit codes.
- **0**: Failures still return 0 or errors are not diagnosable.

### 6) Interactivity, safety, and non-interactive compatibility
- **3**: Non-interactive path is complete (`--no-input`, etc.); high-risk ops require confirmation/force.
- **2**: Interactivity strategy is mostly sound; minor edge gaps remain.
- **1**: CI/scripts may hang, or high-risk safeguards are insufficient.
- **0**: Requires interaction (not automatable), or has clear safety risks.

### 7) Compatibility and evolution strategy
- **3**: No breaking changes, or deprecation + migration path is provided.
- **2**: Minor acceptable changes; migration guidance is basic.
- **1**: Potentially breaking changes without sufficient warnings.
- **0**: Silent breakage of existing behavior/scripts.

### 8) Documentation and test sufficiency
- **3**: Help, examples, and key behavior tests are all updated and cover changes.
- **2**: Either docs or tests have minor gaps.
- **1**: Clear documentation/testing gaps.
- **0**: No doc/test updates.

---

## Blockers (any one triggers `Request Changes`)

- [ ] Missing `-h/--help` or `--version`
- [ ] Severe stdout/stderr mixing breaks machine readability
- [ ] Failure paths return `0`
- [ ] High-risk operations lack confirmation and/or `--force` / `--dry-run`
- [ ] Non-interactive usage can hang on prompts with no alternative arguments
- [ ] Silent breaking change to existing CLI contract (no migration plan)

---

## Suggested decision thresholds

- **Approve**
  - Total score `>= 20` and no blockers
- **Needs Discussion**
  - Total score `16~19` with no blockers, or design trade-offs need team alignment
- **Request Changes**
  - Total score `<= 15` or any blocker exists

> Note: Thresholds can be tuned to project maturity; define them at repo level for consistency.

---

## PR review summary template (copy/paste)

```markdown
### CLI Review Summary

- Total Score: **X / 24**
- Decision: **Approve | Needs Discussion | Request Changes**
- Blockers: **None | N items**

#### Dimension Scores
1. Argument syntax and predictability: X/3
2. Option naming consistency: X/3
3. Help / Version / Discoverability: X/3
4. Output contract (stdout/stderr + machine mode): X/3
5. Error handling and exit codes: X/3
6. Interactivity, safety, and non-interactive compatibility: X/3
7. Compatibility and evolution strategy: X/3
8. Documentation and test sufficiency: X/3

#### Strengths
- ...

#### Issues to Address
- ...

#### Suggested Follow-ups
- ...
```

---

## Related references

- Start with checklist: [checklist.md](checklist.md)
- Resolve naming disputes via: [option-map.md](option-map.md)
- Compare against anti-patterns: [anti-patterns.md](anti-patterns.md)
- Update help text using: [help-template.md](help-template.md)

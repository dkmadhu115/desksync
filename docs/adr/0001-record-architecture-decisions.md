# 1. Record architecture decisions

- Status: Accepted
- Date: 2026-07-23

## Context

DeskSync spans multiple languages and stacks and will be built over many phases
by (potentially) different contributors. We need a lightweight, durable record
of *why* significant decisions were made, not just *what* the code does.

## Decision

We use Architecture Decision Records (ADRs), one Markdown file per decision in
`docs/adr/`, numbered sequentially. Each ADR states context, the decision, and
its consequences. ADRs are immutable once accepted; a superseding ADR is added
rather than editing history.

## Consequences

- New significant decisions (frameworks, protocols, data model, security
  posture) get an ADR in the same PR.
- Reviewers and future maintainers can trace rationale quickly.

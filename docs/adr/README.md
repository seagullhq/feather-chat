# Architecture Decision Records

This folder holds future ADRs: immutable, numbered, one decision per record
(convention per the
[architecture-decision-record](https://github.com/architecture-decision-record/architecture-decision-record)
guidelines).

No records yet — the initial decisions are documented inline in
[`../protocol.md`](../protocol.md), [`../wire-format.md`](../wire-format.md),
and [`../security-model.md`](../security-model.md).

## New ADR

Take the next number (starting from `0001`), title in the form
`NNNN-short-hyphenated-title.md`, template:

```
# ADR NNNN — Title

- **Status:** Proposed | Accepted | Superseded by ADR-MMMM
- **Date:** YYYY-MM-DD

## Context
## Decision
## Consequences
```

Superseded records are never deleted — the new ADR links back; the index
table in this README gains the row.

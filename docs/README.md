# feather-chat docs

Design docs for the project. Read in this order if you are new here.

| Doc | Kind | What it holds |
|---|---|---|
| [`protocol.md`](protocol.md) | Explanation | Protocol model: problem, broad flow, unobvious features |
| [`wire-format.md`](wire-format.md) | **Reference** (normative) | Byte layout, MUST/SHOULD rules, examples, conformance summary |
| [`audio-pipeline.md`](audio-pipeline.md) | Explanation | Client media path (Qt side) |
| [`server-design.md`](server-design.md) | Explanation | Go server structure and integrity rules |
| [`security-model.md`](security-model.md) | Explanation | Threat model, crypto plan, gate for public deployment |
| [`adr/`](adr/) | Reference | Architecture Decision Records, numbered and immutable |

Classification follows Diátaxis: reference states facts, explanation
discusses why. Mixing the two is how documentation dies.

## Rules for docs here

- One doc = one purpose. Byte-level normative statements live only in
  `wire-format.md`; everything else links to it, never duplicates it.
- RFC 2119 keywords ("MUST", "SHOULD") appear only in `wire-format.md`'s
  normative text.
- Design changes go through an ADR once they happen;
  [`adr/README.md`](adr/README.md) carries the convention and template.
- Any edit touching a file listed above keeps this index accurate.

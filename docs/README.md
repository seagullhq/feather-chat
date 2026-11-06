# feather-chat docs

Design docs for the project. Read in this order if you are new here.

| Doc                                      | Kind                      | What it holds                                                 |
| ---------------------------------------- | ------------------------- | ------------------------------------------------------------- |
| [`protocol.md`](protocol.md)             | Explanation               | Protocol model: problem, broad flow, unobvious features       |
| [`wire-format.md`](wire-format.md)       | **Reference** (normative) | Byte layout, MUST/SHOULD rules, examples, conformance summary |
| [`audio-pipeline.md`](audio-pipeline.md) | Explanation               | Client media path (Qt side)                                   |
| [`server-design.md`](server-design.md)   | Explanation               | Go server structure and integrity rules                       |
| [`security-model.md`](security-model.md) | Explanation               | Threat model, crypto plan, gate for public deployment         |

Classification follows Diátaxis: reference states facts, explanation discusses why. Mixing the two is how documentation dies.

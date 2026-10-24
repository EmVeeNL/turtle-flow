# Diagrams

All diagrams are written in [Mermaid](https://mermaid.js.org/) and live **inside the Markdown documents** where they are explained. The `.mmd` files in this directory are extracted copies (generated from the `<!-- diagram: … -->` markers) for tooling such as `mmdc` or editor previews. The Markdown documents are the source of truth.

| # | Diagram | Type | Source document | Standalone file |
|---|---|---|---|---|
| 1 | System architecture | flowchart | [ARCHITECTURE.md §2](../ARCHITECTURE.md#2-system-context) | [01-system-architecture.mmd](01-system-architecture.mmd) |
| 2 | Component architecture | flowchart | [ARCHITECTURE.md §4](../ARCHITECTURE.md#4-component-architecture) | [02-component-architecture.mmd](02-component-architecture.mmd) |
| 3 | Workflow state machine | stateDiagram-v2 | [WORKFLOW.md §3](../WORKFLOW.md#3-job-states) | [03-workflow-state-machine.mmd](03-workflow-state-machine.mmd) |
| 4 | Job execution sequence | sequenceDiagram | [ARCHITECTURE.md §5.1](../ARCHITECTURE.md#51-job-execution-standard-profile-happy-path) | [04-job-execution-sequence.mmd](04-job-execution-sequence.mmd) |
| 5 | Agent routing sequence | sequenceDiagram | [AGENT-PROTOCOL.md §6](../AGENT-PROTOCOL.md#6-provider-selection-and-routing-sequence) | [05-agent-routing-sequence.mmd](05-agent-routing-sequence.mmd) |
| 6 | Token preflight sequence | sequenceDiagram | [TOKEN-INTELLIGENCE.md §6](../TOKEN-INTELLIGENCE.md#6-preflight) | [06-token-preflight-sequence.mmd](06-token-preflight-sequence.mmd) |
| 7 | Project/job/task domain relationships | classDiagram | [DOMAIN-MODEL.md §2](../DOMAIN-MODEL.md#2-entity-relationships) | [07-domain-relationships.mmd](07-domain-relationships.mmd) |
| 8 | Database entity relationship diagram | erDiagram | [DATA-MODEL.md §2](../DATA-MODEL.md#2-entity-relationship-diagram) | [08-database-erd.mmd](08-database-erd.mmd) |
| 9 | Git worktree lifecycle | stateDiagram-v2 | [GIT-STRATEGY.md §3](../GIT-STRATEGY.md#3-worktree-lifecycle) | [09-git-worktree-lifecycle.mmd](09-git-worktree-lifecycle.mmd) |
| 10 | Approval process | flowchart | [WORKFLOW.md §6.5](../WORKFLOW.md#65-approval-process) | [10-approval-process.mmd](10-approval-process.mmd) |
| 11 | Crash recovery process | flowchart | [WORKFLOW.md §10](../WORKFLOW.md#10-recovery-behavior) | [11-crash-recovery.mmd](11-crash-recovery.mmd) |
| 12 | Future server architecture | flowchart | [ROADMAP.md §5](../ROADMAP.md#5-phase-2-turtle-flow-server-architecture) | [12-future-server-architecture.mmd](12-future-server-architecture.mmd) |

Additional unnumbered diagrams: request/response model ([AGENT-PROTOCOL.md §2](../AGENT-PROTOCOL.md#2-requestresponse-model)) and trust boundaries ([SECURITY.md §4](../SECURITY.md#4-trust-boundaries)).

Render all diagrams locally:

```bash
npx -y @mermaid-js/mermaid-cli -i diagrams/03-workflow-state-machine.mmd -o 03-workflow-state-machine.svg -t dark
```

Conventions: diagrams intentionally omit low-level detail (e.g. `blocked`/`cancelled` edges in the state machine are described in the transition table instead) to stay readable. If a diagram and its document disagree, the document's tables are authoritative and the diagram must be fixed.

# Configuration management scope

A document being accepted by PVE State means its structure is valid. It does
not necessarily mean every field is reconciled against production.

The authoritative, machine-readable field registry is generated at
`schemas/management-scope.json` by `pves schema`. Each entry has both a workflow
class and a management level. Levels state the highest lifecycle capability
implemented for that entry:

| Level | Meaning |
| --- | --- |
| `archived` | Retained as capture evidence or inventory; reconciliation is not promised. |
| `declared` | Represented in typed local configuration but not reconciled. |
| `planned` | Compared and emitted as drift, but not adoptable or applicable. |
| `adoptable` | Captured drift can update local configuration, but cannot be applied. |
| `applicable` | Guarded execution exists in the entry's workflow. |

Workflow classes describe where that behavior belongs:

| Class | Meaning |
| --- | --- |
| `production-managed` | Compared with live production and emitted into a guarded apply plan. |
| `recovery-only` | Used by disaster recovery, restoration, or operational validation. |
| `validation-only` | Used only for read-only service checks. |
| `declared-only` | Valid desired configuration that is not currently reconciled. |
| `metadata` | Descriptive inventory or tool compatibility information. |

`declared-only` is intentionally explicit. It prevents a valid YAML document
from being mistaken for an implemented production capability. When support is
added, its registry entry must move to `production-managed` in the same change
as its planner, adoption, executor, and convergence tests, and its level must
move to `applicable`.

At present, notable declared-only areas include selected LXC creation
properties requiring unsafe conversion and descriptive host/storage topology.
Guest existence is also declared-only: captures archive every live guest, but
normal planning does not create missing guests or delete extra guests. Only IDs
declared in `guests.yml` are owned; extra live guests are archived outside that
ownership boundary. Storage definitions and host services follow the same
explicit declared-only boundary.

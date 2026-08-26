# Configuration management scope

A document being accepted by PVE State means its structure is valid. It does
not necessarily mean every field is reconciled against production.

The authoritative, machine-readable field registry is generated at
`schemas/management-scope.json` by `pves schema`. The registry uses these
classes:

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
as its planner, executor, and tests.

At present, notable declared-only areas include selected LXC creation
properties requiring unsafe conversion and descriptive host/storage topology.

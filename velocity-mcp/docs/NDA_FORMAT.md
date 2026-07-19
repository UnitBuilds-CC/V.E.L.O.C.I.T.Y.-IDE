# NDA format contract

This file is the compact format contract agents should use when reading or writing NDA content.

## Purpose

NDA is the canonical internal representation for V.E.L.O.C.I.T.Y. state and semantic content.

Use NDA when you need:
- deterministic structure
- compact semantic representation
- canonical triples or node-oriented content
- low-ambiguity agent output

JSON is not the canonical format here. JSON should be treated as an import/export adapter only.

## Agent rules

When a task requests NDA output:
1. Prefer NDA directly instead of JSON.
2. Preserve semantics exactly; do not add commentary inside the payload.
3. Keep output deterministic and consistently ordered.
4. Use stable identifiers and stable field ordering.
5. Do not mix prose and NDA in the same block unless explicitly requested.
6. If uncertain, produce the smallest valid NDA form that preserves meaning.

## Core shape

NDA should be treated as a semantic, structured format rather than freeform text.

At minimum, agents should assume:
- entities are explicit
- relationships are explicit
- repeated meaning should normalize to the same identifiers
- equivalent content should serialize the same way

For triple-oriented NDA, think in terms of:
- subject
- predicate
- object

## Canonical expectations

A good NDA payload is:
- deterministic
- concise
- lossless for the intended meaning
- easy to diff
- easy to round-trip into adapters

Prefer:
- one canonical representation for one meaning
- explicit relationships over implied relationships
- stable naming over stylistic variation

Avoid:
- redundant wrappers
- decorative formatting
- synonyms that change identifiers unnecessarily
- mixing transport concerns with semantic content

## Practical authoring guidance

When writing NDA:
- emit only the data required by the task
- keep ordering stable across runs
- reuse the same identifier for the same entity
- separate content records cleanly
- avoid hidden assumptions that are not encoded in the data

When reading NDA:
- interpret it semantically, not stylistically
- preserve exact identifiers when transforming it
- do not expand to JSON unless interoperability requires it

## Examples

### Minimal semantic triple set

```text
user:alice knows user:bob
user:alice works_on project:velocity
project:velocity uses format:nda
```

### Canonical entity reuse

```text
task:routing kind refactor
task:routing status planned
task:routing target file:velocity-mcp/src/automation/task_router.rs
```

### Relationship-first planning data

```text
policy:nda-native applies_to task:registry-migration
policy:nda-native canonical_format format:nda
policy:nda-native adapter_format format:json
```

## Common mistakes

Bad patterns:
- adding explanatory prose inside the NDA payload
- renaming the same entity across lines
- representing the same fact in multiple different shapes
- emitting JSON-like wrappers around NDA content
- using non-deterministic ordering

## Response pattern for agents

If the user asks for NDA, prefer this structure:
1. brief prose introduction outside the payload if needed
2. one fenced `text` block containing only NDA
3. optional short note after the block only if clarification is required

Example:

```text
artifact:plan depends_on artifact:sitemap
artifact:plan canonical_format format:nda
artifact:plan export_format format:json
```

## Interop rule

If conversion is required:
- NDA remains the source of truth
- JSON is derived from NDA
- JSON imports should normalize back into canonical NDA

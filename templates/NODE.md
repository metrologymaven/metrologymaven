---
id: PARENT.CHILD
title: Short noun phrase
level: 2
parent: PARENT
kind: subsystem
status: open
phase: P1a
size: L
owners: []
claimed_by:
satisfies: [REQ-PARENT-001]
covers: []
depends_on: []
cli: []
mcp: []
journal: []
updated: YYYY-MM-DD
---

# PARENT.CHILD: Short noun phrase

## Purpose

One paragraph: what this node is responsible for and why it exists as a separate unit.

## Scope

**In:**
- ...

**Out** (and which node owns it instead):
- ... (owned by `OTHER.NODE`)

## Requirements

- **REQ-PARENT.CHILD-001** When <trigger>, the system shall <response>.
- **REQ-PARENT.CHILD-002** The <component> shall <property>.

## Interfaces

What this node offers to others and what it consumes. Name registry commands, data types and events. Above leaf level, patterns are fine.

## Decomposition hints

Suggested children for whoever expands this node. These are not binding.

## Open questions

- ...

## Children

_None yet: status `open`._

<!--
Kinds by level, for guidance: 0 product, 1 domain, 2 subsystem, 3 module, 4 feature, 5+ task.
Frontmatter rules:
- lists are single-line [a, b] and items must not contain commas;
- satisfies: names parent requirement IDs;
- covers: seed feature IDs or patterns (GOM.ALN.*), and must stay within the parent's covers;
- size: S|M are allowed only for leaves; L|XL|XXL must be split.
-->

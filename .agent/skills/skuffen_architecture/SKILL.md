---
name: Skuffen Architecture
description: Architecture documentation and resources for the Skuffen application.
---

# Skuffen Architecture

This skill provides access to architecture documentation, command mappings, and flow diagrams for the Skuffen application.

## Resources

The following resources are available in the `resources` directory:

### Commands and Queries
- **Commands**: [commands.md](resources/command/commands.md) - Documentation for commands (Opprett sak, Opprett Journalpost, etc.) and their operations.
- **Queries**: [query.md](resources/command/query.md) - Documentation for queries (Hent sak, Hent Journalpost, etc.).
- **Visual Mapping**: `resources/command/Skuffen - Kommando mapping til Operasjon.svg`

### Architecture
- **Overview**: `resources/architecture.svg`
- **Design Guidelines**: [design_guidelines.md](resources/architecture/design_guidelines.md) - General info & philosophy.
- **Principles**: [architecture_principles.md](resources/architecture/architecture_principles.md) - Core architecture principles.
- **ID Mapping & Idempotency**: [id_mapping_and_idempotency.md](resources/architecture/id_mapping_and_idempotency.md) - Handling identity and references.

### Flows
Detailed flow diagrams are available in the `resources/flows` directory.

### State Machines
State machine definitions are available in the `resources/state_machines` directory.

## Usage

Use these resources to understand the system's architecture, command/query capabilities, and intended behaviors when implementing features or fixing bugs in Skuffen.

## General Information

See [Design Guidelines](resources/architecture/design_guidelines.md) for general system philosophy and context.

## Architecture principles

See [Architecture Principles](resources/architecture/architecture_principles.md) for detailed architectural rules and the functional core definition.

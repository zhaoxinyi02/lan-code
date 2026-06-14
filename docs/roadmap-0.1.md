# Lan Code 0.1 Acceptance Plan

## Definition of usable

Lan Code 0.1 is usable when the CLI can safely complete common, bounded coding
tasks in an existing Git repository:

1. inspect relevant files without inventing context;
2. create and edit files within the workspace;
3. run explicitly authorized build or test commands;
4. inspect Git status and diff before reporting completion;
5. pause for approval and accept interruption during a turn;
6. recover session history and durable events after restart;
7. never silently replay an uncertain side effect.

## Required before release

- [x] reliable structured multi-file patch tool;
- [x] Git status and diff review loop;
- [x] tool started/completed/failed events;
- [x] basic configuration file with environment-secret references;
- [x] explicit unsandboxed command warning gate (replace with Windows sandbox);
- [x] end-to-end evaluation suite across several small repositories;
- [x] Windows binary packaging and concise setup documentation;

## Explicitly deferred after 0.1

- desktop, VS Code, JetBrains, and web clients;
- multi-user remote runtime;
- MCP and plugin marketplace;
- subagents and background scheduling;
- broad provider catalog.

## Current estimate

The 0.1 acceptance scope is complete. Future hardening should replace the
explicit unsandboxed command gate with OS-level sandbox enforcement and expand
the live-provider evaluation corpus.

---
paths:
  - "crates/harness-core/src/graph/**"
---

# graph — nodex bridge

Read-only wrapper over the `nodex` CLI. `NodexClient<R: NodexRunner>` is
generic over a runner trait so tests substitute a mock that returns
canned JSON envelopes; production uses `DefaultNodexRunner` which spawns
the real binary.

`NodeRef` is loose: well-known fields (`id`, `kind`, `path`, `status`)
are typed; everything else flattens into `extra`. An upstream schema
addition never breaks this bridge.

`parse_items` accepts two envelope shapes (`data: [...]` or
`data: {items: [...]}`) — nodex emits both depending on query.

`DefaultNodexRunner::detect` probes PATH for `nodex --version`. The CLI
command surfaces `GRAPH_SPAWN_FAILURE` if absent — never silently falls
back to grep-based detection. Operators install nodex explicitly.

`diff(ref_a, ref_b)` wraps `nodex diff` and returns the loose
[`GraphDiff`] struct (added/removed nodes/edges + status transitions
typed; the rest under `extra`).

The graph module is also the backbone for `lifecycle`'s `graph-backlinks`
consumer-detection strategy: when configured, `ConsumerDetector` resolves
references via `client.backlinks` instead of walking the filesystem.

When adding a new query:
1. Add a method on `NodexClient` that calls `runner.run(&[...])`.
2. Add a mock-runner test asserting the args passed and the parsed result.
3. Wire into `harness graph` subcommand.
4. If the new query feeds another module (like backlinks feeds lifecycle),
   add an integration point + integration test.

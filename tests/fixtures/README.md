# Fixtures

Shared test data: captured block payloads, project manifests, GraphQL schemas.

Real captured blocks are preferred over hand-written ones wherever a test depends
on the shape of chain data — hand-written fixtures encode what we *believe* an
endpoint returns, and that belief is exactly what a decoding test should be
checking.

Record the chain, height and endpoint alongside anything captured, so it can be
re-fetched and verified later.

# Anchor Indexer Vendor Folder

This folder hosts the anchor-local checkout of simply-kaspa-indexer.

- Upstream checkout path: `AOL2/anchor/indexer/simply-kaspa-indexer`
- Upstream source: `https://github.com/supertypo/simply-kaspa-indexer`
- Intended use here: the anchor live indexer smoke boots this local checkout on `127.0.0.1:8500` when `ANCHOR_TEST_INDEXER_URL` is not supplied.

Runtime expectations:

- if `ANCHOR_TEST_INDEXER_DATABASE_URL` is set, the smoke uses that PostgreSQL instance;
- otherwise it expects Docker so it can launch a temporary local `postgres:16-alpine` container for the bundled indexer;
- if `ANCHOR_TEST_NODE_URL` is set, the bundled indexer points at that wRPC endpoint;
- otherwise the bundled indexer falls back to the Kaspa public node network behavior provided by simply-kaspa-indexer itself.
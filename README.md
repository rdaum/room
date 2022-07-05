What it is/will-be:

 * Stores "slots" a distributed transactional DB (FoundationDB)
 * Services requests to execute "verbs" in slots.
 * "Verbs" are WASM programs.

To run:

 * Install FoundationDB (client and server)
 * `FDB_CLUSTER_FILE=/etc/foundationdb/fdb.cluster RUST_LOG=info cargo run`

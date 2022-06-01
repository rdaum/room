What it is/will-be:

 * Stores MOO-like objects (verbs/properties/prototypes) in a distributed transactional DB (FoundationDB)
 * Services requests to execute "verbs" on those objects over Websockets
 * "Verbs" are WASM programs.

To run:

 * Install FoundationDB (client and server)
 * `FDB_CLUSTER_FILE=/etc/foundationdb/fdb.cluster RUST_LOG=info cargo run`

Currently blows up when trying to execute first "verb" because the calling WASM conventions and memory etc have not been 
properly set up.

To run:

 * Install FoundationDB (client and server)
 * `FDB_CLUSTER_FILE=/etc/foundationdb/fdb.cluster RUST_LOG=info cargo run`

Currently blows up when trying to execute first "verb" because the calling WASM conventions and memory etc have not been 
properly set up.

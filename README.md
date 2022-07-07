#'Room'; a web assembly MOO

## What am I?

This is an attempt to build something akin to classic LambdaMOO/CoolMUD/ColdMUD but on a modern platform.

By "akin to LambdaMOO" I mean:

* A network available service 
* ... which presents a kind of narrative "virtual world" or "metaverse"
* ... which allows for safe, secure, user authoring
* ... with nice user authoring tools
* ... along with a user accessible programming language

By "modern platform" I mean:

* Written with modern tools. In this case: Rust, WebAssembly, and HTTP/WebSockets
* Can hopefully scale out to many thousands of users (as opposed to the "hundreds" of a single classic MOO server.)
* Permits modern programming languages to be used.

## What do I do right now?

* Stores "slots" a distributed transactional DB (FoundationDB for now)
* Executed WebAssembly programs are stored in those slots and which have access to values and other programs stored in those slots.
* Services WebSocket connections whose messages are dispatched to/from programs in those slots.

## What's my 'architecture'?

* An 'engine' (written in Rust, compiled to native) which:
  * hosts a WebAssembly virtual machine.
  * provides the calling ABI and series of 'builtin' functions for the WebAssembly runtime
  * manages requests to the database for 'slot' data reads and writes
  * manages WebSocket connections
* A 'driver' (written in Rust, compiled to WebAssembly) which:
  * receives WebSocket communications
  * implements the higher abstractions that permit user authoring/object creation
  * implements higher level user scripting languages (custom or off the shelf?) for user programming
* A 'world' (written in the user scripting PL) which:
  * Has "rooms", "things", "players", and a narrative stream.
  * Presents a rich UI of the above.

## What's actually done?

* The foundations of the engine.
* The very start of the driver.

## What's still to be done?

* Most things.

# To run:

 * Install FoundationDB (client and server)
 * `cargo make build` from workspace root
 * From 'engine'; `FDB_CLUSTER_FILE=/etc/foundationdb/fdb.cluster RUST_LOG=info cargo run`

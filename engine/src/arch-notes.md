# Maybe? this is mostly fantasy especially the 'kernel' level.

# Architecture notes

a distributed shared program world

kernel <-> driver <-> world

# A sketch

peeling the onion we start on the outside with a modified and advanced MOO-like environment:

on the outside ('**world**')

  * a shared narrative world that users can alter
  * synchronous communication
  * ability to write user programs in a friendly authoring script(s)

a layer down (the '**driver**')

  * the environment that _provides_ that outside world 'skin' written in the environment itself
  * assemblyscript environment providing fundamental services that allow the world to exist:
    * objects/prototypes, verbs, properties, capabilities
  * orthogonal persistence of a WASM environment

and beneath that (the '**kernel**')

  * physical primitive transactional DB layer that stores primitive WASM types
  * websocket & HTTP server
  * WASM VM

# Layers in more detail

from the inside out this time

## kernel
* A transactional db storage & lowest level data model
  * db & cache 
  * stores keys (oid+name) -> slots of fundamental assemblyscript/wasm types + vector types
  * facilities for unpacking/packing these tuples
    * for storage into foundationdb, but also for passing to/from wasm
  * db has simple mapping keys -> tuples
  * secondary/tertiary indices & joins
* wasm vm engine
  * executes WASM programs in sandboxes
  * ABIs for passing WASM arguments to/from host?
* network / websocket facility
  * dispatches websocket payloads to/from WASM layer via a declared 
    'receive' function in WASM (in driver layer) 

## driver
* assemblyscript compiler
* object layer written in assemblyscript and held in 'image'
    * prototypes, "verbs", etc.
    * take up something like the 'mica' model i had before:
      * prototype multiple dispatch with keyword selector arguments
* capabilities manager
* programs written in assemblyscript providing services to users
* user language interpreter(s)
  
## world
* fundamental prototypes: rooms, things, etc.


# getting there from here.

mostly a rewrite,

* rip out most of rewrite object/fdb_object/world/fdb_world
* ditch 'connection object' piece, websocket layer speaks to only one WASM program
* use WASI?
* get assemblyscript compiler/environment in



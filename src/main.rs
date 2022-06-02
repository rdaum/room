use futures::StreamExt;
use log::*;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use tungstenite::{Message, Result};

pub mod fdb_object;
pub mod fdb_world;
pub mod object;
pub mod vm;
pub mod wasm_vm;
pub mod world;

async fn handle_message<'world_lifetime>(
    conn_oid: object::Oid,
    msg: Result<Message>,
    world: Arc<dyn world::World + Send + Sync + 'world_lifetime>,
) {
    match msg {
        Ok(m) => {
            if m.is_text() || m.is_binary() {
                world
                    .receive(conn_oid, m)
                    .await
                    .expect("Could not receive message");
            }
        }
        Err(e) => match e {
            tungstenite::Error::Protocol(_) | tungstenite::Error::ConnectionClosed => {
                error!("Closed, deleting {:?}", conn_oid);
                world
                    .destroy_object(conn_oid)
                    .await
                    .expect("Unable to destroy connection object");
            }
            _ => {}
        },
    }
}

async fn handle_connection<'world_lifetime>(
    peer: SocketAddr,
    stream: TcpStream,
    world: Arc<dyn world::World + Send + Sync + 'world_lifetime>,
) -> tungstenite::Result<()> {
    let ws_stream = accept_async(stream).await.expect("Failed to accept");

    let conn_oid = world
        .clone()
        .create_connection_object()
        .await
        .expect("Failed to create connection object");
    info!("New WebSocket connection: {} to OID {:?}", peer, conn_oid);

    let (mut _outgoing, mut incoming) = ws_stream.split();
    loop {
        tokio::select! {
            msg = incoming.next() => {
                match msg {
                    Some(msg) => {
                        handle_message(conn_oid, msg, world.clone()).await;
                    }
                    None => break,
                }
            }
        }
    }

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let vm = wasm_vm::WasmVM::new();
    let world = fdb_world::FdbWorld::new(vm);
    match world.initialize_world().await {
        Ok(()) => {
            info!("World initialized.")
        }
        Err(e) => {
            panic!("Could not initialize world: {:?}", e);
        }
    }

    let addr = "127.0.0.1:9002";
    let listener = TcpListener::bind(&addr).await.expect("Can't listen");
    info!("Listening on: {}", addr);
    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream
            .peer_addr()
            .expect("connected streams should have a peer address");
        info!("Peer address: {}", peer);

        tokio::spawn(handle_connection(peer, stream, world.clone()));
    }

    Ok(())
}

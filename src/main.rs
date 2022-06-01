use futures::StreamExt;
use log::*;
use std::error::Error;
use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use tungstenite::Result;

pub mod object;
pub mod world;
pub mod fdb_object;

async fn accept_connection<'world_lifetime>(
    peer: SocketAddr,
    stream: TcpStream,
    world: Arc<world::World<'world_lifetime>>,
) {
    if let Err(e) = handle_connection(peer, stream, world).await {
        match e {
            err => error!("Error processing connection: {}", err),
        }
    }
}

async fn handle_connection<'world_lifetime>(
    peer: SocketAddr,
    stream: TcpStream,
    world: Arc<world::World<'world_lifetime>>,
) -> tungstenite::Result<()> {
    let mut ws_stream = accept_async(stream).await.expect("Failed to accept");

    let conn_oid = world
        .create_connection_object()
        .await
        .expect("Failed to create connection object");
    info!("New WebSocket connection: {} to OID {:?}", peer, conn_oid);

    while let Some(msg) = ws_stream.next().await {
        match msg {
            Ok(msg) => {
                if msg.is_text() || msg.is_binary() {
                    world
                        .receive(conn_oid, msg)
                        .await
                        .expect("Could not call receive");
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
                _ => {
                    return Err(e);
                }
            },
        }
    }
    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    env_logger::init();

    let world = Arc::new(world::World::new());
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

        tokio::spawn(accept_connection(peer, stream, world.clone()));
    }

    Ok(())
}

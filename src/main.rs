use bytes::Bytes;
use clap::Parser;
use futures::{future, pin_mut, StreamExt};
use futures_channel::mpsc::unbounded;
use log::*;
use std::{error::Error, net::SocketAddr, sync::Arc};
use tokio::net::{TcpListener, TcpStream};
use tokio_tungstenite::accept_async;
use tungstenite::{Message, Result};
use crate::world::{register_connection, disconnect, initialize_world, receive_connection_message};

pub mod fdb_object;
pub mod world;
pub mod object;

pub mod vm;
pub mod wasm_vm;

#[derive(Parser, Debug)]
#[clap(author, version, about, long_about = None)]
struct Args {
    /// Listen address to bind the websocket server to.
    #[clap(short, long, default_value = "127.0.0.1:9002")]
    listen_address: String,
}

async fn handle_message(
    conn_oid: object::Oid,
    msg: Result<Message>,
    world:  Arc<world::World>,
) {
    match msg {
        Ok(m) => {
            if m.is_text() || m.is_binary() {
                // Consume message and pass off to receive..
                let message = Bytes::from(m.into_data());

                receive_connection_message(&world, conn_oid, message)
                    .await
                    .expect("Could not receive message");
            }
        }
        Err(e) => match e {
            tungstenite::Error::Protocol(_) | tungstenite::Error::ConnectionClosed => {
                error!("Closed, deleting {:?}", conn_oid);
                    disconnect(world, conn_oid)
                    .await
                    .expect("Unable to destroy connection object");
            }
            _ => {}
        },
    }
}

async fn handle_connection(
    peer: SocketAddr,
    stream: TcpStream,
    world: Arc<world::World>,
) -> tungstenite::Result<()> {
    let ws_stream = accept_async(stream).await.expect("Failed to accept");

    // Create an unbounded channel stream from tx->rx and let the world own the tx.
    let (tx, rx) = unbounded();
    let conn_oid = register_connection(world.clone(), tx, peer)
        .await
        .expect("Failed to create connection object");
    info!("New WebSocket connection: {} to OID {:?}", peer, conn_oid);

    // Split the stream into inbound/outbound...
    let (outgoing, incoming) = ws_stream.split();

    // Create a future to forward messages from 'rx' into the outbound.
    let receive_forward = rx.map(Ok).forward(outgoing);

    // And create a future to handle inbound messages.
    let process_incoming = incoming.for_each(|msg| async {
        handle_message(conn_oid, msg, world.clone()).await;
    });

    pin_mut!(process_incoming, receive_forward);

    // Perform the selection on both inbound/outbound.
    future::select(receive_forward, process_incoming).await;

    Ok(())
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error>> {
    let args = Args::parse();

    env_logger::init();

    let world = Arc::new(world::World::new());

    match initialize_world(world.clone()).await {
        Ok(()) => {
            info!("World initialized.")
        }
        Err(e) => {
            panic!("Could not initialize world: {:?}", e);
        }
    }

    let listener = TcpListener::bind(args.listen_address.clone())
        .await
        .expect("Can't listen");
    info!("Listening on: {}", args.listen_address.clone());
    while let Ok((stream, _)) = listener.accept().await {
        let peer = stream
            .peer_addr()
            .expect("connected streams should have a peer address");
        info!("Peer address: {}", peer);

        tokio::spawn(handle_connection(peer, stream, world.clone()));
    }

    Ok(())
}

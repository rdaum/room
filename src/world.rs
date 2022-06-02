use futures::channel::mpsc::UnboundedSender;
use futures::future::BoxFuture;
use std::error::Error;
use std::net::SocketAddr;

use tungstenite::Message;

use crate::object::Oid;

pub trait World {
    /// Initialize the world, getting it ready for use.
    fn initialize(&self) -> BoxFuture<Result<(), Box<dyn Error>>>;

    /// Notify the world of an inbound websocket connection and return the Oid for its connection.
    ///
    /// * `sender` can be used by the World to send messages to the connection
    /// * `address` the client socket address of the connection
    fn connect(
        &self,
        sender: UnboundedSender<Message>,
        address: SocketAddr,
    ) -> BoxFuture<Result<Oid, Box<dyn Error>>>;

    /// Notify the world that a websocket connection has disconnected.
    ///
    /// * `oid` the connection object associated with the disconnected session
    fn disconnect(&self, oid: Oid) -> BoxFuture<Result<(), Box<dyn Error>>>;

    /// Notify the world that an inbound websocket message has been received for a given connection.
    ///
    /// * `connection` the connection object associated with the websocket that sent the messages
    /// * `message` the inbound message
    fn receive(&self, connection: Oid, message: Message) -> BoxFuture<Result<(), Box<dyn Error>>>;
}

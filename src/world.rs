use futures::channel::mpsc::UnboundedSender;
use futures::future::BoxFuture;
use std::error::Error;
use std::net::SocketAddr;

use tungstenite::Message;

use crate::object::Oid;

pub trait World {
    fn receive(&self, connection: Oid, message: Message) -> BoxFuture<Result<(), Box<dyn Error>>>;
    fn create_connection_object(
        &self,
        sender: UnboundedSender<Message>,
        address: SocketAddr,
    ) -> BoxFuture<Result<Oid, Box<dyn Error>>>;
    fn destroy_object(&self, oid: Oid) -> BoxFuture<Result<(), Box<dyn Error>>>;
    fn initialize_world(&self) -> BoxFuture<Result<(), Box<dyn Error>>>;
}

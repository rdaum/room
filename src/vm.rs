use futures::future::BoxFuture;

use crate::{object::Method, world::World};

pub trait VM {
    fn execute_method(
        &self,
        method: &Method,
        world: &(dyn World + Send + Sync),
    ) -> BoxFuture<Result<(), anyhow::Error>>;
}

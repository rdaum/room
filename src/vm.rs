use futures::future::BoxFuture;

use crate::{object::Method, object::Value, world::World};

pub trait VM {
    fn execute_method(
        &self,
        method: &Method,
        world: &(dyn World + Send + Sync),
        args: &Value,
    ) -> BoxFuture<Result<(), anyhow::Error>>;
}

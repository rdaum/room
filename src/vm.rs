use futures::future::BoxFuture;

use crate::{object::Program, object::Value, world::World};

pub trait VM {
    fn execute(
        &self,
        program: &Program,
        world: &(dyn World + Send + Sync),
        args: &Value,
    ) -> BoxFuture<Result<(), anyhow::Error>>;
}

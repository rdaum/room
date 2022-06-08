use std::sync::Arc;
use futures::future::BoxFuture;
use futures::lock::Mutex;

use crate::{object::Program, object::Value};
use crate::world::World;

pub trait VM {
    fn execute(
        &self,
        program: &Program,
        world: Arc<World>,
        args: &Value,
    ) -> BoxFuture<Result<(), anyhow::Error>>;
}

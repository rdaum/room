use std::error::Error;

use crate::{object::Method, world::World};

pub trait VM {
    fn execute_method(&self, method: &Method, world: &dyn World) -> Result<(), Box<dyn Error>>;
}

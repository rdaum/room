use std::error::Error;

use crate::object::Method;

pub trait VM {
    fn execute_method(&self, method: &Method) -> Result<(), Box<dyn Error>>;
}

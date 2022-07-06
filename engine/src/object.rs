use value::{Error, Oid, Value};

/// An "object" is purely a bag of "slots". It does not necessarily represent an 'object' in the
/// same terminology as an object-oriented programming language, but rather just a collection of
/// attributes.
/// That is, an object has no inheritance, no delegation, no class, etc.
/// Each "slot" is identified by its location, a visibility key, and a string name.
/// In this manner a generic object model is defined upon which others can be built in the runtime.
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};

/// The definition of a slot on an object.
#[derive(Clone, Debug, Eq, Hash, PartialEq, Serialize, Deserialize)]
pub struct SlotDef {
    pub location: Oid,
    pub key: Oid,
    pub name: String,
}

/// Associate OIDs with slots.
/// Objects are bags of slots.
pub trait ObjDBHandle {
    /// Set a slot on an object
    ///
    /// * `location` what object to set the slot on
    /// * `key` A unique ID which masks visibility on the slot.
    /// * `name` the name of the slot
    /// * `value` the value of the slot
    fn set_slot(&self, location: Oid, key: Oid, name: String, value: &Value);

    /// Get a slot from an object
    ///
    /// * `location` what object to get the slot from
    /// * `key` The unique ID which masks visibility on the slot.
    /// * `name` the name of the slot
    fn get_slot(&self, location: Oid, key: Oid, name: String) -> BoxFuture<Result<Value, Error>>;

    /// Find all slots defined for an object
    ///
    /// * `location` what object to get the slot from
    /// * 'key' key visibilty mask
    fn get_slots(
        &self,
        location: Oid,
        key: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = SlotDef> + Send + Unpin>, Error>;
}

pub trait AdminHandle {
    fn dump_slots(
        &self,
        location: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = (SlotDef, Value)> + Send + Unpin>, Error>;
}

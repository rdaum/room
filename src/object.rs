use futures::future::BoxFuture;
use int_enum::IntEnum;
use serde::{Deserialize, Serialize};

/// An "object" is purely a bag of "slots". It does not necessarily represent an 'object' in the
/// same terminology as an object-oriented programming language, but rather just a collection of
/// attributes.
/// That is, an object has no inheritance, no delegation, no class, etc.
/// Each "slot" is identified by its location, a visibility key, and a string name.
/// In this manner a generic object model is defined upon which others can be built in the runtime.

// An Oid is 128-bit V4 UUID.
// Used to identify objects & keys on objects.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Oid {
    pub id: uuid::Uuid,
}

/// The definition of a slot on an object.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct SlotDef {
    pub location: Oid,
    pub key: Oid,
    pub name: String,
}

#[repr(i8)]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq, IntEnum)]
pub enum ValueType {
    I32 = 0, // Mapping
    I64 = 1,
    F32 = 2,
    F64 = 3,
    V128 = 4,
    String = 5,  // UTF-8 strings
    IdKey = 6,   // Refs to Objects
    Vector = 7,  // Collections of Values
    Binary = 8,  // Byte arrays
    Program = 9, // WAS code,
    Error = 10,
}

pub type Program = Vec<u8>;

#[repr(i8)]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, IntEnum)]
pub enum Error {
    SlotDoesNotExist = 0,
    InvalidProgram = 1,
    PermissionDenied = 2,
    InternalError = 3,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Value {
    I32(i32),
    I64(i64),
    F32(f32),
    F64(f64),
    V128(u128),
    String(String),
    Vector(Vec<Value>),
    Binary(Vec<u8>),
    Program(Program),
    IdKey(Oid),
    Error(Error),
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
    fn get_slots(
        &self,
        location: Oid,
        key: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = SlotDef> + Send + Unpin>, Error>;
}

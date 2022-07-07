use int_enum::IntEnum;
use serde::{Deserialize, Serialize};

// An Oid is 128-bit V4 UUID.
// Used to identify objects & keys on objects.
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Oid {
    pub id: uuid::Uuid,
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
#[derive(Serialize, Deserialize, Clone, Copy, Debug, PartialEq, Eq, IntEnum)]
pub enum Error {
    NoError = 0,
    SlotDoesNotExist = 1,
    InvalidProgram = 2,
    PermissionDenied = 3,
    InternalError = 4,
    BadType = 5,
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

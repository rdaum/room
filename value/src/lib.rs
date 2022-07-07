use bytes::buf::{Buf, BufMut};
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
    U128(u128),
    String(String),
    Vector(Vec<Value>),
    Binary(Vec<u8>),
    Program(Program),
    IdKey(Oid),
    Error(Error),
}

pub fn parse_value(buf: &mut dyn Buf) -> Value {
    let type_val_idx = buf.get_i8();
    let tval = ValueType::from_int(type_val_idx).unwrap();
    match tval {
        ValueType::I32 => {
            let num = buf.get_i32();
            Value::I32(num)
        }
        ValueType::I64 => {
            let num = buf.get_i64();
            Value::I64(num)
        }
        ValueType::F32 => {
            let num = buf.get_f32();
            Value::F32(num)
        }
        ValueType::F64 => {
            let num = buf.get_f64();
            Value::F64(num)
        }
        ValueType::V128 => Value::U128(buf.get_u128()),
        ValueType::String => {
            let len = buf.get_u32() as usize;
            let mut dst_bytes: Vec<u8> = Vec::with_capacity(len);
            dst_bytes.resize(len, 0);
            buf.copy_to_slice(dst_bytes.as_mut_slice());
            Value::String(String::from_utf8(dst_bytes).unwrap())
        }
        ValueType::IdKey => {
            let oid_bytes = buf.get_u128();
            Value::IdKey(Oid {
                id: uuid::Uuid::from_u128(oid_bytes),
            })
        }
        ValueType::Vector => {
            let size: usize = buf.get_u32() as usize;
            let mut l_val: Vec<Value> = Vec::with_capacity(size);
            for _n in 0..size {
                l_val.push(parse_value(buf));
            }
            Value::Vector(l_val)
        }
        ValueType::Binary => {
            let len = buf.get_u32() as usize;
            let mut dst_bytes: Vec<u8> = Vec::with_capacity(len);
            dst_bytes.resize(len, 0);
            buf.copy_to_slice(dst_bytes.as_mut_slice());
            Value::Binary(dst_bytes)
        }
        ValueType::Program => {
            let len = buf.get_u32() as usize;
            let mut dst_bytes: Vec<u8> = Vec::with_capacity(len);
            dst_bytes.resize(len, 0);
            buf.copy_to_slice(dst_bytes.as_mut_slice());
            Value::Program(dst_bytes)
        }
        ValueType::Error => {
            let num = buf.get_i8();
            Value::Error(Error::from_int(num).unwrap())
        }
    }
}

pub fn append_value(buf: &mut Vec<u8>, val: &Value) {
    match &val {
        Value::I32(v) => {
            buf.put_i8(ValueType::I32 as i8);
            buf.put_i32(*v);
        }
        Value::I64(v) => {
            buf.put_i8(ValueType::I64 as i8);
            buf.put_i64(*v);
        }
        Value::F32(v) => {
            buf.put_i8(ValueType::F32 as i8);
            buf.put_f32(*v);
        }
        Value::F64(v) => {
            buf.put_i8(ValueType::F64 as i8);
            buf.put_f64(*v);
        }
        Value::U128(v) => {
            buf.put_i8(ValueType::V128 as i8);
            buf.put_u128(*v);
        }
        Value::String(s) => {
            buf.put_i8(ValueType::String as i8);
            buf.put_u32(s.len() as u32);
            let str_bytes = s.as_bytes();
            buf.put(str_bytes);
        }
        Value::IdKey(u) => {
            buf.put_i8(ValueType::IdKey as i8);
            buf.put_u128(u.id.as_u128());
        }
        Value::Vector(v) => {
            buf.put_i8(ValueType::Vector as i8);
            buf.put_u32(v.len() as u32);
            for i in v {
                append_value(buf, i);
            }
        }
        Value::Binary(b) => {
            buf.put_i8(ValueType::Binary as i8);
            buf.put_u32(b.len() as u32);
            buf.put(b.to_owned().as_slice());
        }
        Value::Program(b) => {
            buf.put_i8(ValueType::Binary as i8);
            buf.put_u32(b.len() as u32);
            buf.put(b.to_owned().as_slice());
        }
        Value::Error(err) => {
            buf.put_i8(ValueType::Error as i8);
            buf.put_i8(*err as i8);
        }
    }
}

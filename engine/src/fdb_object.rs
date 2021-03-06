use assert_str::assert_str_eq;
use bytes::Bytes;

use fdb::{
    range::RangeOptions,
    subspace::Subspace,
    transaction::{FdbTransaction, ReadTransaction, Transaction},
    tuple::Tuple,
    Key,
};
use futures::future::{BoxFuture, FutureExt};
use int_enum::IntEnum;

use tokio_stream::StreamExt;

use crate::object::{AdminHandle, ObjDBHandle, SlotDef};
use value::{Error, Oid, Value, ValueType};

pub trait RangeKey {
    fn list_start_key(location: Oid, definer: Oid) -> Tuple;
    fn list_end_key(location: Oid, definer: Oid) -> Tuple;
}

// Wrap some types so we can implement conversion to/from keys traits for it.
pub struct FdbOid(pub Oid);
pub struct FdbValue(pub Value);

impl From<fdb::Key> for FdbOid {
    fn from(key: Key) -> Self {
        let oid_subspace = Subspace::new(Bytes::from_static("OID".as_bytes()));
        let bytes: Bytes = key.into();
        assert!(oid_subspace.contains(&bytes));
        let tuple = oid_subspace.unpack(&bytes).unwrap();
        FdbOid(Oid {
            id: *tuple.get_uuid_ref(0).unwrap(),
        })
    }
}

impl From<FdbOid> for fdb::Key {
    fn from(oid: FdbOid) -> Self {
        let oid_subspace = Subspace::new(Bytes::from_static("OID".as_bytes()));
        let mut tuple = Tuple::new();
        tuple.add_uuid(oid.0.id);
        oid_subspace.subspace(&tuple).pack().into()
    }
}

impl From<SlotDef> for fdb::Key {
    fn from(slotdef: SlotDef) -> Self {
        let slotdef_subspace = Subspace::new(Bytes::from_static("SLOT".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(slotdef.location.id);
        tup.add_uuid(slotdef.key.id);
        tup.add_string(slotdef.name);
        slotdef_subspace.subspace(&tup).pack().into()
    }
}

impl From<fdb::Key> for SlotDef {
    fn from(key: fdb::Key) -> Self {
        let slotdef_subspace = Subspace::new(Bytes::from_static("SLOT".as_bytes()));
        let bytes: Bytes = key.into();
        let tuple = slotdef_subspace.unpack(&bytes).unwrap();
        SlotDef {
            location: Oid {
                id: *tuple.get_uuid_ref(0).unwrap(),
            },
            key: Oid {
                id: *tuple.get_uuid_ref(1).unwrap(),
            },
            name: tuple.get_string_ref(2).unwrap().clone(),
        }
    }
}

impl From<&Tuple> for FdbValue {
    fn from(tuple: &Tuple) -> Self {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("VALUE"));
        let type_val_idx = tuple.get_i8(1).unwrap();

        let tval = ValueType::from_int(type_val_idx).unwrap();
        match tval {
            ValueType::I32 => {
                let num = tuple.get_i32(2).unwrap();
                FdbValue(Value::I32(num))
            }
            ValueType::I64 => {
                let num = tuple.get_i64(2).unwrap();
                FdbValue(Value::I64(num))
            }
            ValueType::F32 => {
                let num = tuple.get_f32(2).unwrap();
                FdbValue(Value::F32(num))
            }
            ValueType::F64 => {
                let num = tuple.get_f64(2).unwrap();
                FdbValue(Value::F64(num))
            }
            ValueType::V128 => {
                let b = tuple.get_bytes_ref(2).unwrap();
                // this feels wrong
                let c = [
                    b[0], b[1], b[2], b[3], b[4], b[5], b[6], b[7], b[8], b[9], b[10], b[11],
                    b[12], b[13], b[14], b[15],
                ];
                FdbValue(Value::U128(u128::from_be_bytes(c)))
            }
            ValueType::String => {
                let str = tuple.get_string_ref(2).unwrap();
                FdbValue(Value::String(str.clone()))
            }
            ValueType::IdKey => {
                let oid = tuple.get_uuid_ref(2).unwrap();
                FdbValue(Value::IdKey(Oid { id: *oid }))
            }
            ValueType::Vector => {
                let size: usize = tuple.get_i32(2).unwrap() as usize;
                let mut l_val: Vec<Value> = vec![];
                for n in 0..size {
                    let t = tuple.get_tuple_ref(3_usize + n).unwrap();
                    let v: FdbValue = t.into();
                    l_val.push(v.0);
                }
                FdbValue(Value::Vector(l_val))
            }
            ValueType::Binary => {
                let bytes = tuple.get_bytes_ref(2).unwrap();
                FdbValue(Value::Binary(bytes.to_vec()))
            }
            ValueType::Program => {
                let bytes = tuple.get_bytes_ref(2).unwrap();
                FdbValue(Value::Program(bytes.to_vec()))
            }
            ValueType::Error => {
                let num = tuple.get_i8(2).unwrap();
                FdbValue(Value::Error(Error::from_int(num).unwrap()))
            }
        }
    }
}

impl From<fdb::Value> for FdbValue {
    fn from(value: fdb::Value) -> Self {
        let tuple = &Tuple::from_bytes(value).unwrap();
        tuple.into()
    }
}

impl From<&FdbValue> for Tuple {
    fn from(vwrap: &FdbValue) -> Self {
        let value = &vwrap.0;
        let mut tup = Tuple::new();
        tup.add_string(String::from("VALUE"));
        match &value {
            Value::I32(v) => {
                tup.add_i8(ValueType::I32 as i8);
                tup.add_i32(*v);
            }
            Value::I64(v) => {
                tup.add_i8(ValueType::I64 as i8);
                tup.add_i64(*v);
            }
            Value::F32(v) => {
                tup.add_i8(ValueType::F32 as i8);
                tup.add_f32(*v);
            }
            Value::F64(v) => {
                tup.add_i8(ValueType::F64 as i8);
                tup.add_f64(*v);
            }
            Value::U128(v) => {
                tup.add_i8(ValueType::V128 as i8);
                let be_bytes = Bytes::from(v.to_be_bytes().to_vec());
                tup.add_bytes(be_bytes);
            }
            Value::String(s) => {
                tup.add_i8(ValueType::String as i8);
                tup.add_string(s.clone());
            }
            Value::IdKey(u) => {
                tup.add_i8(ValueType::IdKey as i8);
                tup.add_uuid(u.id);
            }
            Value::Vector(v) => {
                tup.add_i8(ValueType::Vector as i8);
                tup.add_i32(v.len() as i32);
                for i in v {
                    let tuple: Tuple = FdbValue(i.clone()).into();
                    tup.add_tuple(tuple);
                }
            }
            Value::Binary(b) => {
                tup.add_i8(ValueType::Binary as i8);
                tup.add_bytes(Bytes::from(b.clone()));
            }
            Value::Program(b) => {
                tup.add_i8(ValueType::Program as i8);
                tup.add_bytes(Bytes::from(b.clone()));
            }
            Value::Error(err) => {
                tup.add_i8(ValueType::Error as i8);
                tup.add_i8(*err as i8);
            }
        }
        tup
    }
}

impl From<FdbValue> for Tuple {
    fn from(v: FdbValue) -> Self {
        v.into()
    }
}

impl From<&FdbValue> for fdb::Value {
    fn from(vwrap: &FdbValue) -> Self {
        let tup: Tuple = vwrap.into();
        tup.pack().into()
    }
}

// Performs operations on objects via one transaction.
pub struct ObjDBTxHandle<'tx_lifetime> {
    tr: &'tx_lifetime FdbTransaction,
}

impl<'tx_lifetime> ObjDBTxHandle<'tx_lifetime> {
    pub fn new(tx: &'tx_lifetime FdbTransaction) -> Self {
        ObjDBTxHandle { tr: tx }
    }
}

impl<'tx_lifetime> ObjDBHandle for ObjDBTxHandle<'tx_lifetime> {
    fn set_slot(&self, location: Oid, definer: Oid, name: String, value: &Value) {
        let slotdef = SlotDef {
            location,
            key: definer,
            name,
        };
        self.tr.set(slotdef, &FdbValue(value.clone()));
    }

    fn get_slot(
        &self,
        location: Oid,
        definer: Oid,
        name: String,
    ) -> BoxFuture<Result<Value, Error>> {
        async move {
            let slotdef = SlotDef {
                location,
                key: definer,
                name,
            };
            let result_future = self.tr.get(slotdef).await;

            match result_future {
                Ok(result) => match result {
                    None => Err(Error::SlotDoesNotExist),
                    Some(r) => {
                        let v: FdbValue = r.into();
                        Ok(v.0)
                    }
                },
                Err(_) => Err(Error::InternalError),
            }
        }
        .boxed()
    }

    fn get_slots(
        &self,
        location: Oid,
        key: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = SlotDef> + Send + Unpin>, Error> {
        let slotdef_subspace = Subspace::new(Bytes::from_static("SLOT".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(location.id);
        tup.add_uuid(key.id);
        let slot_range = slotdef_subspace.range(&tup);
        let range_stream = slot_range.into_stream(self.tr, RangeOptions::default());
        let slotdefs = range_stream.map(|kv| -> SlotDef {
            let key = kv.unwrap().get_key_ref().clone();

            SlotDef::from(key)
        });
        Ok(Box::new(slotdefs))
    }
}

impl<'tx_lifetime> AdminHandle for ObjDBTxHandle<'tx_lifetime> {
    fn dump_slots(
        &self,
        location: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = (SlotDef, Value)> + Send + Unpin>, Error> {
        let slotdef_subspace = Subspace::new(Bytes::from_static("SLOT".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(location.id);
        let slot_range = slotdef_subspace.range(&tup);
        let range_stream = slot_range.into_stream(self.tr, RangeOptions::default());
        let slotdefs = range_stream.map(|kv| -> (SlotDef, Value) {
            let kv = kv.unwrap();
            let key = kv.get_key_ref().clone();
            let val = kv.get_value_ref().clone();

            (SlotDef::from(key), FdbValue::from(val).0)
        });
        Ok(Box::new(slotdefs))
    }
}

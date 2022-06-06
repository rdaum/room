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
use log::{error, info};
use tokio_stream::StreamExt;

use crate::object::{
    Method, ObjDBHandle, ObjGetError, Object, Oid, PropDef, PropGetError, Value, ValueType,
    VerbDef, VerbGetError,
};

pub trait RangeKey {
    fn list_start_key(location: Oid, definer: Oid) -> Tuple;
    fn list_end_key(location: Oid, definer: Oid) -> Tuple;
}

impl From<fdb::Key> for Oid {
    fn from(key: Key) -> Self {
        let oid_subspace = Subspace::new(Bytes::from_static("OID".as_bytes()));
        let bytes: Bytes = key.into();
        assert!(oid_subspace.contains(&bytes));
        let tuple = oid_subspace.unpack(&bytes).unwrap();
        Oid {
            id: *tuple.get_uuid_ref(0).unwrap(),
        }
    }
}

impl From<Oid> for fdb::Key {
    fn from(oid: Oid) -> Self {
        let oid_subspace = Subspace::new(Bytes::from_static("OID".as_bytes()));
        let mut tuple = Tuple::new();
        tuple.add_uuid(oid.id);
        oid_subspace.subspace(&tuple).pack().into()
    }
}

impl From<PropDef> for fdb::Key {
    fn from(propdef: PropDef) -> Self {
        let propdef_subspace = Subspace::new(Bytes::from_static("PROPDEF".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(propdef.location.id);
        tup.add_uuid(propdef.definer.id);
        tup.add_string(propdef.name);
        propdef_subspace.subspace(&tup).pack().into()
    }
}

impl From<fdb::Key> for PropDef {
    fn from(key: fdb::Key) -> Self {
        let propdef_subspace = Subspace::new(Bytes::from_static("PROPDEF".as_bytes()));
        let bytes: Bytes = key.into();
        let tuple = propdef_subspace.unpack(&bytes).unwrap();
        PropDef {
            location: Oid {
                id: *tuple.get_uuid_ref(0).unwrap(),
            },
            definer: Oid {
                id: *tuple.get_uuid_ref(1).unwrap(),
            },
            name: tuple.get_string_ref(2).unwrap().clone(),
        }
    }
}

impl From<VerbDef> for fdb::Key {
    fn from(vd: VerbDef) -> Self {
        let verbdef_subspace = Subspace::new(Bytes::from_static("VERBDEF".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(vd.definer.id);
        tup.add_string(vd.name);
        verbdef_subspace.subspace(&tup).pack().into()
    }
}

impl From<fdb::Key> for VerbDef {
    fn from(key: fdb::Key) -> Self {
        let verbdef_subspace = Subspace::new(Bytes::from_static("VERBDEF".as_bytes()));
        let bytes: Bytes = key.into();
        let tuple = verbdef_subspace.unpack(&bytes).unwrap();
        VerbDef {
            definer: Oid {
                id: *tuple.get_uuid_ref(0).unwrap(),
            },
            name: tuple.get_string_ref(1).unwrap().clone(),
        }
    }
}

impl From<&Tuple> for Value {
    fn from(tuple: &Tuple) -> Self {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("VALUE"));
        let type_val_idx = tuple.get_i8(1).unwrap();

        let tval = ValueType::from_int(type_val_idx).unwrap();
        match tval {
            ValueType::String => {
                let str = tuple.get_string_ref(2).unwrap();
                Value::String(str.clone())
            }
            ValueType::Number => {
                let num = tuple.get_f64(2).unwrap();
                Value::Number(num)
            }
            ValueType::Obj => {
                let oid = tuple.get_uuid_ref(2).unwrap();
                Value::Obj(Oid { id: *oid })
            }
            ValueType::List => {
                let size: usize = tuple.get_i32(2).unwrap() as usize;
                let mut l_val: Vec<Value> = vec![];
                for n in 0..size {
                    let t = tuple.get_tuple_ref(3_usize + n).unwrap();
                    let v: Value = t.into();
                    l_val.push(v);
                }
                Value::List(l_val)
            }
            ValueType::Binary => {
                let bytes = tuple.get_bytes_ref(2).unwrap();
                Value::Binary(bytes.to_vec())
            }
        }
    }
}

impl From<fdb::Value> for Value {
    fn from(value: fdb::Value) -> Self {
        let tuple = &Tuple::from_bytes(value).unwrap();
        tuple.into()
    }
}

impl From<&Value> for Tuple {
    fn from(value: &Value) -> Self {
        let mut tup = Tuple::new();
        tup.add_string(String::from("VALUE"));
        match &value {
            Value::String(s) => {
                tup.add_i8(ValueType::String as i8);
                tup.add_string(s.clone());
            }
            Value::Number(n) => {
                tup.add_i8(ValueType::Number as i8);
                tup.add_f64(*n);
            }
            Value::Obj(u) => {
                tup.add_i8(ValueType::Obj as i8);
                tup.add_uuid(u.id);
            }
            Value::List(v) => {
                tup.add_i8(ValueType::List as i8);
                tup.add_i32(v.len() as i32);
                for i in v {
                    let tuple: Tuple = i.into();
                    tup.add_tuple(tuple);
                }
            }
            Value::Binary(b) => {
                tup.add_bytes(Bytes::from(b.clone()));
            }
        }
        tup
    }
}

impl From<&Value> for fdb::Value {
    fn from(value: &Value) -> Self {
        let tup: Tuple = value.into();
        tup.pack().into()
    }
}

impl From<&Method> for fdb::Value {
    fn from(m: &Method) -> Self {
        let mut tup = Tuple::new();
        tup.add_bytes(m.method.clone());
        tup.pack().into()
    }
}

impl From<fdb::Value> for Method {
    fn from(bytes: fdb::Value) -> Self {
        let tuple = Tuple::from_bytes(bytes).unwrap();
        Method {
            method: tuple.get_bytes_ref(0).unwrap().clone(),
        }
    }
}

impl From<fdb::Value> for Object {
    fn from(bytes: fdb::Value) -> Self {
        let object_subspace = Subspace::new(Bytes::from_static("OBJECT".as_bytes()));
        let tuple = object_subspace.unpack(&bytes.into()).unwrap();
        let mut obj = Object {
            oid: Oid {
                id: *tuple.get_uuid_ref(0).unwrap(),
            },
            delegates: vec![],
        };
        let mut offset = 1;
        let num_delegates = tuple.get_i32(offset).unwrap();
        offset += 1;
        for _delegate_num in 0..num_delegates {
            let delegate_id = tuple.get_uuid_ref(offset).unwrap();
            obj.delegates.push(Oid { id: *delegate_id });
            offset += 1;
        }
        obj
    }
}

impl From<&Object> for fdb::Value {
    fn from(o: &Object) -> Self {
        let object_subspace = Subspace::new(Bytes::from_static("OBJECT".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(o.oid.id);
        tup.add_i32(o.delegates.len() as i32);
        for delegate in o.delegates.iter() {
            tup.add_uuid(delegate.id);
        }
        object_subspace.subspace(&tup).pack().into()
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
    fn put(&self, oid: Oid, obj: &Object) {
        self.tr.set(oid, obj)
    }

    fn get(&self, oid: Oid) -> BoxFuture<Result<Object, ObjGetError>> {
        async move {
            let result_future = self.tr.get(oid).await;

            match result_future {
                Ok(result) => match result {
                    None => Err(ObjGetError::DoesNotExist()),
                    Some(r) => Ok(r.into()),
                },
                Err(_) => Err(ObjGetError::DbError()),
            }
        }
        .boxed()
    }

    fn put_verb(&self, definer: Oid, name: String, method: &Method) {
        let verbdef = VerbDef { definer, name };
        self.tr.set(verbdef, method);
    }

    fn get_verb(&self, definer: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>> {
        async move {
            let verbdef = VerbDef { definer, name };
            let result_future = self.tr.get(verbdef).await;

            match result_future {
                Ok(result) => match result {
                    None => Err(VerbGetError::DoesNotExist()),
                    Some(r) => Ok(r.into()),
                },
                Err(_) => Err(VerbGetError::DbError()),
            }
        }
        .boxed()
    }

    fn find_verb(&self, location: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>> {
        // Look locally first.
        async move {
            let local_look = self.get_verb(location, name.clone()).await;

            match local_look {
                Ok(r) => Ok(r),
                Err(e) if e == VerbGetError::DoesNotExist() => {
                    // Get delegates list.
                    let o_look = self.get(location).await;
                    let delegates = match o_look {
                        Ok(o) => { o.delegates }
                        Err(e) => match e {
                            ObjGetError::DbError() => {
                                error!("Unable to retrieve object to retrieve delegates list due db error: {:?}", e);
                                return Err(VerbGetError::DbError());
                            }
                            ObjGetError::DoesNotExist() => {
                                error!("Unable to retrieve object to retrieve delegates list due to invalid delegate ({:?}): {:?}", location, e);
                                vec![]
                            }
                        }
                    };

                    info!("Delegates for obj {:?} == {:?}", location, delegates);

                    // Depth first search up delegate tree.
                    for delegate in delegates {
                        match self.find_verb(delegate, name.clone()).await {
                            Ok(o) => return Ok(o),
                            Err(e) if e == VerbGetError::DoesNotExist() => continue,
                            Err(e) => return Err(e),
                        }
                    }
                    Err(VerbGetError::DoesNotExist())
                }

                Err(e) => Err(e),
            }
        }
            .boxed()
    }

    fn set_property(&self, location: Oid, definer: Oid, name: String, value: &Value) {
        let propdef = PropDef {
            location,
            definer,
            name,
        };
        self.tr.set(propdef, value);
    }

    fn get_property(
        &self,
        location: Oid,
        definer: Oid,
        name: String,
    ) -> BoxFuture<Result<Value, PropGetError>> {
        async move {
            let propdef = PropDef {
                location,
                definer,
                name,
            };
            let result_future = self.tr.get(propdef).await;

            match result_future {
                Ok(result) => match result {
                    None => Err(PropGetError::DoesNotExist()),
                    Some(r) => Ok(r.into()),
                },
                Err(_) => Err(PropGetError::DbError()),
            }
        }
        .boxed()
    }

    fn get_properties(
        &self,
        location: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = PropDef> + Send + Unpin>, PropGetError> {
        let propdef_subspace = Subspace::new(Bytes::from_static("PROPDEF".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(location.id);
        let prop_range = propdef_subspace.range(&tup);
        let range_stream = prop_range.into_stream(self.tr, RangeOptions::default());
        let propdefs = range_stream.map(|kv| -> PropDef {
            let key = kv.unwrap().get_key_ref().clone();

            PropDef::from(key)
        });
        Ok(Box::new(propdefs))
    }

    fn get_verbs(
        &self,
        location: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = VerbDef> + Send + Unpin>, VerbGetError> {
        let verbdef_subspace = Subspace::new(Bytes::from_static("VERBDEF".as_bytes()));
        let mut tup = Tuple::new();
        tup.add_uuid(location.id);
        let verb_range = verbdef_subspace.range(&tup);
        let range_stream = verb_range.into_stream(self.tr, RangeOptions::default());
        let verbdefs = range_stream.map(|kv| -> VerbDef {
            let key = kv.unwrap().get_key_ref().clone();

            VerbDef::from(key)
        });
        Ok(Box::new(verbdefs))
    }
}

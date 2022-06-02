use assert_str::assert_str_eq;

use fdb::range::RangeOptions;
use fdb::transaction::{FdbTransaction, ReadTransaction, Transaction};
use fdb::tuple::Tuple;
use fdb::KeySelector;
use futures::future::{BoxFuture, FutureExt};
use futures::StreamExt;
use int_enum::IntEnum;
use uuid::Uuid;

use crate::object::{
    Method, ObjDBHandle, ObjGetError, Object, Oid, PropDef, PropGetError, Value, ValueType,
    VerbDef, VerbGetError,
};

pub trait Serialize {
    fn to_tuple(&self) -> Tuple;
    fn from_tuple(tuple: &Tuple) -> Self;
}

pub trait RangeKey {
    fn list_start_key(location: Oid, definer: Oid) -> Tuple;
    fn list_end_key(location: Oid, definer: Oid) -> Tuple;
}

impl Serialize for PropDef {
    fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("PROPDEF"));
        tup.add_uuid(self.location.id);
        tup.add_uuid(self.definer.id);
        tup.add_string(self.name.clone());
        tup
    }

    fn from_tuple(tuple: &Tuple) -> PropDef {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("PROPDEF"));
        PropDef {
            location: Oid {
                id: *tuple.get_uuid_ref(2).unwrap(),
            },
            definer: Oid {
                id: *tuple.get_uuid_ref(3).unwrap(),
            },
            name: tuple.get_string_ref(1).unwrap().clone(),
        }
    }
}

impl RangeKey for PropDef {
    fn list_start_key(location: Oid, definer: Oid) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("PROPDEF"));
        tup.add_uuid(location.id);
        tup.add_uuid(definer.id);

        println!("Start: {:?}", tup);
        tup
    }

    fn list_end_key(location: Oid, definer: Oid) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("PROPDEF"));
        let increment_location = Uuid::from_u128(location.id.as_u128() + 1);
        tup.add_uuid(increment_location);
        tup.add_uuid(definer.id);

        println!("End: {:?}", tup);
        tup
    }
}

impl Serialize for VerbDef {
    fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("VERBDEF"));
        tup.add_string(self.name.clone());
        tup.add_uuid(self.definer.id);
        tup
    }

    fn from_tuple(tuple: &Tuple) -> VerbDef {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("VERBDEF"));
        VerbDef {
            name: tuple.get_string_ref(1).unwrap().clone(),
            definer: Oid {
                id: *tuple.get_uuid_ref(2).unwrap(),
            },
        }
    }
}

impl Serialize for Value {
    fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("VALUE"));
        match &self {
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
        }
        tup
    }

    fn from_tuple(tuple: &Tuple) -> Value {
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
        }
    }
}

impl Serialize for Method {
    fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_bytes(self.method.clone());
        tup
    }

    fn from_tuple(tuple: &Tuple) -> Method {
        Method {
            method: tuple.get_bytes_ref(0).unwrap().clone(),
        }
    }
}

impl Serialize for Object {
    fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("OBJECT"));
        tup.add_uuid(self.oid.id);
        tup.add_i32(self.delegates.len() as i32);
        for delegate in self.delegates.iter() {
            tup.add_uuid(delegate.id);
        }
        tup
    }

    fn from_tuple(tuple: &Tuple) -> Object {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("OBJECT"));
        let mut obj = Object {
            oid: Oid {
                id: *tuple.get_uuid_ref(1).unwrap(),
            },
            delegates: vec![],
        };
        let mut offset = 2;
        let num_delegates = tuple.get_i32(offset).unwrap();
        offset += 1;
        for _delegate_num in 1..num_delegates {
            let delegate_id = tuple.get_uuid_ref(offset).unwrap();
            obj.delegates.push(Oid {
                id: *delegate_id,
            });
            offset += 1;
        }
        obj
    }
}

// Performs operations on objects via one transaction.
pub struct ObjDBTxHandle<'tx_lifetime> {
    tr: &'tx_lifetime FdbTransaction,
}

// TODO this should be refactored to use a trait. except traits can't have async functions right
// now.
impl<'tx_lifetime> ObjDBTxHandle<'tx_lifetime> {
    pub fn new(
        tx: &'tx_lifetime FdbTransaction,
    ) -> Box<dyn ObjDBHandle + Sync + Send + 'tx_lifetime> {
        Box::new(ObjDBTxHandle { tr: tx })
    }
}

impl<'tx_lifetime> ObjDBHandle for ObjDBTxHandle<'tx_lifetime> {
    fn put(&self, oid: Oid, obj: &Object) {
        let mut oid_tup = Tuple::new();
        oid_tup.add_uuid(oid.id);

        self.tr.set(oid_tup.pack(), obj.to_tuple().pack())
    }

    fn get(&self, oid: Oid) -> BoxFuture<Result<Object, ObjGetError>> {
        async move {
            let mut oid_tup = Tuple::new();
            oid_tup.add_uuid(oid.id);
            let result_future = self.tr.get(oid_tup.pack()).await;

            match result_future {
                Ok(result) => match result {
                    None => Err(ObjGetError::DoesNotExist()),
                    Some(r) => Ok(Object::from_tuple(&Tuple::from_bytes(r).unwrap())),
                },
                Err(_) => Err(ObjGetError::DbError()),
            }
        }
        .boxed()
    }

    fn put_verb(&self, definer: Oid, name: String, value: &Method) {
        let verbdef = VerbDef {
            definer,
            name,
        };
        let verbdef_key = verbdef.to_tuple();
        let value_tuple = value.clone().to_tuple();
        self.tr.set(verbdef_key.pack(), value_tuple.pack());
    }

    fn get_verb(&self, definer: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>> {
        async move {
            let verbdef = VerbDef {
                definer,
                name,
            };
            let verbdef_key = verbdef.to_tuple();
            let result_future = self.tr.get(verbdef_key.pack()).await;

            match result_future {
                Ok(result) => match result {
                    None => Err(VerbGetError::DoesNotExist()),
                    Some(r) => Ok(Method::from_tuple(&Tuple::from_bytes(r).unwrap())),
                },
                Err(_) => Err(VerbGetError::DbError()),
            }
        }
        .boxed()
    }

    // TODO does not work, needs debugging.
    fn find_verb(&self, definer: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>> {
        // Look locally first.
        async move {
            let local_look = self.get_verb(definer, name.clone()).await;

            match local_look {
                Ok(r) => Ok(r),

                Err(e) if e == VerbGetError::DoesNotExist() => {
                    // Get delegates list.
                    let o_look = self.get(definer).await;
                    match o_look {
                        Ok(o) => {
                            // Depth first search up delegate tree.
                            for delegate in o.delegates {
                                // TODO possible to do this in parallel. Explore. Probably not much
                                // value since more than 1 delegate would be rare.
                                match self.find_verb(delegate, name.clone()).await {
                                    Ok(o) => return Ok(o),
                                    Err(e) if e == VerbGetError::DoesNotExist() => continue,
                                    Err(e) => return Err(e),
                                }
                            }
                            Err(VerbGetError::DoesNotExist())
                        }
                        Err(e) => match e {
                            ObjGetError::DbError() => Err(VerbGetError::DbError()),
                            ObjGetError::DoesNotExist() => Err(VerbGetError::Internal),
                        },
                    }
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
        let propdef_key = propdef.to_tuple();
        let value_tuple = value.clone().to_tuple();
        self.tr.set(propdef_key.pack(), value_tuple.pack());
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
            let propdef_key = propdef.to_tuple();
            let result_future = self.tr.get(propdef_key.pack()).await;

            match result_future {
                Ok(result) => match result {
                    None => Err(PropGetError::DoesNotExist()),
                    Some(r) => Ok(Value::from_tuple(&Tuple::from_bytes(r).unwrap())),
                },
                Err(_) => Err(PropGetError::DbError()),
            }
        }
        .boxed()
    }

    // TODO does not work. Something wrong with the range query
    fn get_properties(
        &self,
        location: Oid,
        definer: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = PropDef>>, PropGetError> {
        let start_key = PropDef::list_start_key(location, definer).pack();
        let end_key = PropDef::list_end_key(location, definer).pack();
        let ks_start = KeySelector::first_greater_or_equal(start_key);
        let ks_end = KeySelector::last_less_or_equal(end_key);
        let range = self.tr.get_range(ks_start, ks_end, RangeOptions::default());
        println!("Range: {:?}", range);
        let propdefs = range.map(|kv| -> PropDef {
            let key = kv.unwrap().get_key_ref().clone();
            let key_tuple = Tuple::from_bytes(key);
            print!("PD: {:?}", key_tuple);

            PropDef::from_tuple(&key_tuple.unwrap())
        });
        Ok(Box::new(propdefs))
    }
}

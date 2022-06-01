use assert_str::assert_str_eq;
use bytes::Bytes;
use fdb::error::FdbError;
use fdb::range::RangeOptions;
use fdb::transaction::{FdbTransaction, ReadTransaction, Transaction};
use fdb::tuple::Tuple;
use fdb::KeySelector;
use futures::future::{BoxFuture, FutureExt};
use futures::StreamExt;
use int_enum::IntEnum;
use uuid::Uuid;

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Oid {
    pub id: uuid::Uuid,
}

// The definition of a property slot on an object.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PropDef {
    pub location: Oid,
    pub definer: Oid,
    pub name: String,
}

impl PropDef {
    pub fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("PROPDEF"));
        tup.add_uuid(self.location.id.clone());
        tup.add_uuid(self.definer.id.clone());
        tup.add_string(self.name.clone());
        tup
    }

    pub fn from_tuple(tuple: &Tuple) -> PropDef {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("PROPDEF"));
        PropDef {
            location: Oid {
                id: tuple.get_uuid_ref(2).unwrap().clone(),
            },
            definer: Oid {
                id: tuple.get_uuid_ref(3).unwrap().clone(),
            },
            name: tuple.get_string_ref(1).unwrap().clone(),
        }
    }

    pub fn list_start_key(location: Oid, definer: Oid) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("PROPDEF"));
        tup.add_uuid(location.id.clone());
        tup.add_uuid(definer.id.clone());

        println!("Start: {:?}", tup);
        tup
    }

    pub fn list_end_key(location: Oid, definer: Oid) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("PROPDEF"));
        let increment_location = Uuid::from_u128(location.id.as_u128() + 1);
        tup.add_uuid(increment_location.clone());
        tup.add_uuid(definer.id.clone());

        println!("End: {:?}", tup);
        tup
    }
}

#[repr(i8)]
#[derive(Clone, Copy, Debug, Eq, PartialEq, IntEnum)]
pub enum ValueType {
    String = 0,
    Number = 1,
    Obj = 2,
}

#[derive(Clone, Debug)]
pub enum Value {
    String(String),
    Number(f64),
    Obj(Oid),
}

impl Value {
    pub fn to_tuple(self: Value) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("VALUE"));
        match &self {
            Value::String(s) => {
                tup.add_i8(ValueType::String as i8);
                tup.add_string(s.clone());
            }
            Value::Number(n) => {
                tup.add_i8(ValueType::Number as i8);
                tup.add_f64(n.clone());
            }
            Value::Obj(u) => {
                tup.add_i8(ValueType::Obj as i8);
                tup.add_uuid(u.id.clone());
            }
        }
        tup
    }

    pub fn from_tuple(tuple: &Tuple) -> Value {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("VALUE"));
        let type_val_idx = tuple.get_i8(1).unwrap();

        let tval = ValueType::from_int(type_val_idx).unwrap();
        match tval {
            ValueType::String => {
                let str = tuple.get_string_ref(2).unwrap();
                return Value::String(str.clone());
            }
            ValueType::Number => {
                let num = tuple.get_f64(2).unwrap();
                return Value::Number(num.clone());
            }
            ValueType::Obj => {
                let oid = tuple.get_uuid_ref(2).unwrap();
                return Value::Obj(Oid { id: oid.clone() });
            }
        }
    }
}

#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VerbDef {
    definer: Oid,
    name: String,
}

impl VerbDef {
    pub fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("VERBDEF"));
        tup.add_string(self.name.clone());
        tup.add_uuid(self.definer.id.clone());
        tup
    }

    pub fn from_tuple(tuple: &Tuple) -> VerbDef {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("VERBDEF"));
        VerbDef {
            name: tuple.get_string_ref(1).unwrap().clone(),
            definer: Oid {
                id: tuple.get_uuid_ref(2).unwrap().clone(),
            },
        }
    }
}

#[derive(Clone, Debug)]
pub struct Method {
    pub method: Bytes,
}

impl Method {
    pub fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_bytes(self.method.clone());
        tup
    }

    pub fn from_tuple(tuple: &Tuple) -> Method {
        Method {
            method: tuple.get_bytes_ref(0).unwrap().clone(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct Object {
    pub oid: Oid,
    pub delegates: Vec<Oid>,
}

impl Object {
    fn to_tuple(&self) -> Tuple {
        let mut tup = Tuple::new();
        tup.add_string(String::from("OBJECT"));
        tup.add_uuid(self.oid.id.clone());
        tup.add_i32(self.delegates.len() as i32);
        for delegate in self.delegates.iter() {
            tup.add_uuid(delegate.id.clone());
        }
        tup
    }

    fn from_tuple(tuple: &Tuple) -> Object {
        assert_str_eq!(tuple.get_string_ref(0).unwrap(), String::from("OBJECT"));
        let mut obj = Object {
            oid: Oid {
                id: tuple.get_uuid_ref(1).unwrap().clone(),
            },
            delegates: vec![],
        };
        let mut offset = 2;
        let num_delegates = tuple.get_i32(offset).unwrap();
        offset = offset + 1;
        for _delegate_num in 1..num_delegates {
            let delegate_id = tuple.get_uuid_ref(offset).unwrap();
            obj.delegates.push(Oid {
                id: delegate_id.clone(),
            });
            offset = offset + 1;
        }
        obj
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum ObjGetError {
    DbError(FdbError),
    DoesNotExist(),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropGetError {
    DbError(FdbError),
    DoesNotExist(),
}

#[derive(Clone, Debug, PartialEq)]
pub enum VerbGetError {
    DbError(FdbError),
    DoesNotExist(),
    Internal,
}

// Performs operations on objects via one transaction.
pub struct ObjDBTxHandle<'tx_lifetime> {
    tr: &'tx_lifetime FdbTransaction,
}

// TODO this should be refactored to use a trait. except traits can't have async functions right
// now.
impl<'tx_lifetime> ObjDBTxHandle<'tx_lifetime> {
    pub fn new(tx: &'tx_lifetime FdbTransaction) -> ObjDBTxHandle<'tx_lifetime> {
        ObjDBTxHandle { tr: tx }
    }

    pub fn put(&self, oid: Oid, obj: &Object) -> () {
        let mut oid_tup = Tuple::new();
        oid_tup.add_uuid(oid.id);

        self.tr.set(oid_tup.pack(), obj.to_tuple().pack())
    }

    pub async fn get(&self, oid: Oid) -> Result<Object, ObjGetError> {
        let mut oid_tup = Tuple::new();
        oid_tup.add_uuid(oid.id);
        let result_future = self.tr.get(oid_tup.pack()).await;

        match result_future {
            Ok(result) => match result {
                None => Err(ObjGetError::DoesNotExist()),
                Some(r) => Ok(Object::from_tuple(&Tuple::from_bytes(r).unwrap())),
            },
            Err(e) => Err(ObjGetError::DbError(e)),
        }
    }

    pub fn add_verb(&self, definer: Oid, name: String, value: &Method) {
        let verbdef = VerbDef {
            definer: definer,
            name: name,
        };
        let verbdef_key = verbdef.to_tuple();
        let value_tuple = value.clone().to_tuple();
        self.tr.set(verbdef_key.pack(), value_tuple.pack());
    }

    pub async fn get_verb(&self, definer: Oid, name: String) -> Result<Method, VerbGetError> {
        let verbdef = VerbDef {
            definer: definer,
            name: name,
        };
        let verbdef_key = verbdef.to_tuple();
        let result_future = self.tr.get(verbdef_key.pack()).await;

        match result_future {
            Ok(result) => match result {
                None => Err(VerbGetError::DoesNotExist()),
                Some(r) => Ok(Method::from_tuple(&Tuple::from_bytes(r).unwrap())),
            },
            Err(e) => Err(VerbGetError::DbError(e)),
        }
    }

    // TODO does not work, needs debugging.
    pub fn find_verb(&self, definer: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>> {
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
                            return Err(VerbGetError::DoesNotExist());
                        }
                        Err(e) => match e {
                            ObjGetError::DbError(dberr) => Err(VerbGetError::DbError(dberr)),
                            ObjGetError::DoesNotExist() => Err(VerbGetError::Internal),
                        },
                    }
                }

                Err(e) => return Err(e),
            }
        }
        .boxed()
    }

    pub fn set_property(&self, location: Oid, definer: Oid, name: String, value: &Value) {
        let propdef = PropDef {
            location: location,
            definer: definer,
            name: name,
        };
        let propdef_key = propdef.to_tuple();
        let value_tuple = value.clone().to_tuple();
        self.tr.set(propdef_key.pack(), value_tuple.pack());
    }

    pub async fn get_property(
        &self,
        location: Oid,
        definer: Oid,
        name: String,
    ) -> Result<Value, PropGetError> {
        let propdef = PropDef {
            location: location,
            definer: definer,
            name: name,
        };
        let propdef_key = propdef.to_tuple();
        let result_future = self.tr.get(propdef_key.pack()).await;

        match result_future {
            Ok(result) => match result {
                None => Err(PropGetError::DoesNotExist()),
                Some(r) => Ok(Value::from_tuple(&Tuple::from_bytes(r).unwrap())),
            },
            Err(e) => Err(PropGetError::DbError(e)),
        }
    }

    // TODO does not work. Something wrong with the range query
    pub fn get_properties(
        &self,
        location: Oid,
        definer: Oid,
    ) -> Result<impl tokio_stream::Stream<Item = PropDef>, PropGetError> {
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

            return PropDef::from_tuple(&key_tuple.unwrap().clone());
        });
        Ok(propdefs)
    }
}

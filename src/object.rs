use bytes::Bytes;
use futures::future::BoxFuture;
use int_enum::IntEnum;

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


#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct VerbDef {
    pub definer: Oid,
    pub name: String,
}

#[derive(Clone, Debug)]
pub struct Method {
    pub method: Bytes,
}


#[derive(Clone, Debug)]
pub struct Object {
    pub oid: Oid,
    pub delegates: Vec<Oid>,
}


#[derive(Clone, Debug, PartialEq)]
pub enum ObjGetError {
    DbError(),
    DoesNotExist(),
}

#[derive(Clone, Debug, PartialEq)]
pub enum PropGetError {
    DbError(),
    DoesNotExist(),
}

#[derive(Clone, Debug, PartialEq)]
pub enum VerbGetError {
    DbError(),
    DoesNotExist(),
    Internal,
}

pub trait ObjDBHandle {
    fn put(&self, oid: Oid, obj: &Object) -> ();
    fn get(&self, oid: Oid) -> BoxFuture<Result<Object, ObjGetError>>;
    fn add_verb(&self, definer: Oid, name: String, value: &Method);
    fn get_verb(&self, definer: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>>;
    fn find_verb(&self, definer: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>>;
    fn set_property(&self, location: Oid, definer: Oid, name: String, value: &Value);
    fn get_property(
        &self,
        location: Oid,
        definer: Oid,
        name: String,
    ) -> BoxFuture<Result<Value, PropGetError>>;
    fn get_properties(
        &self,
        location: Oid,
        definer: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = PropDef>>, PropGetError>;
}


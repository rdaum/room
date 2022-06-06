use bytes::Bytes;
use futures::future::BoxFuture;
use int_enum::IntEnum;
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub struct Oid {
    pub id: uuid::Uuid,
}

/// The definition of a property slot on an object.
#[derive(Clone, Debug, Eq, Hash, PartialEq)]
pub struct PropDef {
    pub location: Oid,
    pub definer: Oid,
    pub name: String,
}

#[repr(i8)]
#[derive(Serialize, Deserialize, Clone, Copy, Debug, Eq, PartialEq, IntEnum)]
pub enum ValueType {
    String = 0,
    Number = 1,
    Obj = 2,
    List = 3,
    Binary = 4,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
pub enum Value {
    String(String),
    Number(f64),
    Obj(Oid),
    List(Vec<Value>),
    Binary(Vec<u8>),
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

/// A handle to the object DB, through which all object access must go.
/// A new instance per transaction.
pub trait ObjDBHandle {
    /// Create or update an object.
    ///
    /// * `oid` - the unique identifier for the object
    /// * `obj` - the full object record
    fn put(&self, oid: Oid, obj: &Object);

    /// Retrieve an object
    ///
    /// * `oid` - the unique identifier for the object to be retrieved
    fn get(&self, oid: Oid) -> BoxFuture<Result<Object, ObjGetError>>;

    /// Add or update verb on an object
    ///
    /// * `definer` where the verb should be defined
    /// * `name` the unique identifier of the verb
    /// * `value` the body of the verb
    fn put_verb(&self, definer: Oid, name: String, value: &Method);

    /// Retrieve a verb on a specific object, without doing a search across delegates.
    ///
    /// * `definer` what object to look on
    /// * `name` the name of the verb to look for
    fn get_verb(&self, definer: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>>;

    /// Find a verb across the entire ancestry graph for a given object. Depth first search.
    ///
    /// * `location` what object to start looking at
    /// * `name` the name of the verb to look for
    fn find_verb(&self, location: Oid, name: String) -> BoxFuture<Result<Method, VerbGetError>>;

    /// Set a property on an object
    ///
    /// * `location` what object to set the property on
    /// * `definer` what object 'defines' the property (what verbs it is visible to)
    /// * `name` the name of the property
    /// * `value` the value of the property
    fn set_property(&self, location: Oid, definer: Oid, name: String, value: &Value);

    /// Get a property from an object
    ///
    /// * `location` what object to get the property from
    /// * `definer` what object 'defines' the property (what verbs it is visible to)
    /// * `name` the name of the property
    fn get_property(
        &self,
        location: Oid,
        definer: Oid,
        name: String,
    ) -> BoxFuture<Result<Value, PropGetError>>;

    /// Find all properties defined on an object
    ///
    /// * `location` what object to get the properties from
    fn get_properties(
        &self,
        location: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = PropDef> + Send + Unpin>, PropGetError>;

    /// Find all verbs defined on an object
    ///
    /// * `location` what object to get the properties from
    fn get_verbs(
        &self,
        location: Oid,
    ) -> Result<Box<dyn tokio_stream::Stream<Item = VerbDef> + Send + Unpin>, VerbGetError>;
}

use num_traits::{One, Unsigned, Zero};

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::ops::Add;

pub trait TypedSled {
    fn get_val<T: DeserializeOwned>(&self, key: impl AsRef<[u8]>) -> Option<T>;
    fn set_val<T: Serialize>(&self, key: impl AsRef<[u8]>, val: T);
    fn increment<T>(&self, key: impl AsRef<[u8]>) -> T
    where
        T: Add + DeserializeOwned + Serialize + One + Zero + Unsigned,
        <T as Add>::Output: Serialize;
}

impl TypedSled for sled::Tree {
    fn get_val<T: DeserializeOwned>(&self, key: impl AsRef<[u8]>) -> Option<T> {
        self.get(key)
            .unwrap()
            .map(|bytes| bincode::deserialize(&bytes).unwrap())
    }
    fn set_val<T: Serialize>(&self, key: impl AsRef<[u8]>, val: T) {
        let bytes = bincode::serialize(&val).unwrap();
        self.insert(key, bytes).unwrap().unwrap();
    }
    /// increment the value in the db or insert zero if none has been set
    fn increment<T>(&self, key: impl AsRef<[u8]>) -> T
    where
        T: Add + DeserializeOwned + Serialize + One + Zero + Unsigned,
        <T as Add>::Output: Serialize,
    {
        let bytes = self
            .update_and_fetch(key, increment::<T>)
            .unwrap()
            .expect("increment inserts zero if no value is set");
        bincode::deserialize(&bytes).unwrap()
    }
}

fn increment<T>(old: Option<&[u8]>) -> Option<Vec<u8>>
where
    T: Add + DeserializeOwned + Serialize + One + Zero + Unsigned,
    <T as Add>::Output: Serialize,
{
    let new = if let Some(bytes) = old {
        let number: T = bincode::deserialize(bytes).unwrap();
        number + T::one()
    } else {
        T::zero()
    };

    let bytes = bincode::serialize(&new).unwrap();
    Some(bytes)
}

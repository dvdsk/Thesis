use num_traits::{One, Unsigned, Zero};

use serde::de::DeserializeOwned;
use serde::Serialize;
use std::fmt::Debug;
use std::ops::Add;
use tracing::trace;

pub struct CompareAndSwapError<T> {
    pub current: Option<T>,
    pub proposed: Option<T>,
}

impl<T: DeserializeOwned> From<sled::CompareAndSwapError> for CompareAndSwapError<T> {
    fn from(e: sled::CompareAndSwapError) -> Self {
        Self {
            current: e
                .current
                .map(|b| bincode::deserialize(&b))
                .map(Result::unwrap),
            proposed: e
                .proposed
                .map(|b| bincode::deserialize(&b))
                .map(Result::unwrap),
        }
    }
}

pub trait TypedSled {
    fn get_val<T: DeserializeOwned + Debug>(&self, key: impl AsRef<[u8]>) -> Option<T>;
    fn set_val<T: Serialize + Debug>(&self, key: impl AsRef<[u8]>, val: T);
    /// Set value only if key had no value.
    /// # Error
    /// if key was set return the current vale
    fn set_unique<T: Serialize + DeserializeOwned + Debug>(
        &self,
        key: impl AsRef<[u8]>,
        val: T,
    ) -> Result<(), T>;
    fn cas<T: Serialize + DeserializeOwned + Debug>(
        &self,
        key: impl AsRef<[u8]>,
        old: T,
        new: T,
    ) -> Result<(), CompareAndSwapError<T>>;
    fn increment<T>(&self, key: impl AsRef<[u8]>) -> T
    where
        T: Add + DeserializeOwned + Serialize + One + Zero + Unsigned,
        <T as Add>::Output: Serialize;
    fn update<T>(&self, key: impl AsRef<[u8]>, func: fn(T) -> T) -> Option<T>
    where
        T: DeserializeOwned + Serialize;
}

impl TypedSled for sled::Tree {
    /// returns None if there was no value for the given key
    fn get_val<T: DeserializeOwned + Debug>(&self, key: impl AsRef<[u8]>) -> Option<T> {
        let bytes = self.get(key).unwrap();
        let val = bytes
            .as_ref()
            .map(|bytes| bincode::deserialize(bytes).expect("something went wrong deserializing"));
        trace!("deserializing: {val:?} from {bytes:?}");
        val
    }
    fn set_val<T: Serialize + Debug>(&self, key: impl AsRef<[u8]>, val: T) {
        let bytes = bincode::serialize(&val).unwrap();
        trace!("serializing: {val:?} as {bytes:?}");
        let _ig_old_key = self.insert(key, bytes).unwrap();
    }
    fn set_unique<T: Serialize + DeserializeOwned + Debug>(
        &self,
        key: impl AsRef<[u8]>,
        val: T,
    ) -> Result<(), T> {
        let bytes = bincode::serialize(&val).unwrap();
        trace!("serializing: {val:?} as {bytes:?}");
        self.compare_and_swap(key, None as Option<&[u8]>, Some(bytes))
            .unwrap()
            .map_err(|err| err.current)
            .map_err(Option::unwrap)
            .map_err(|bytes| bincode::deserialize(&bytes))
            .map_err(Result::unwrap)
    }

    fn cas<T: Serialize + DeserializeOwned + Debug>(
        &self,
        key: impl AsRef<[u8]>,
        old: T,
        new: T,
    ) -> Result<(), CompareAndSwapError<T>> {
        let old = bincode::serialize(&old).unwrap();
        let new = bincode::serialize(&new).unwrap();
        self.compare_and_swap(key, Some(old), Some(new))
            .unwrap()
            .map_err(|err| err.into())
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
        bincode::deserialize(&bytes).expect("something went wrong deserializing")
    }
    fn update<T>(&self, key: impl AsRef<[u8]>, func: fn(T) -> T) -> Option<T>
    where
        T: DeserializeOwned + Serialize,
    {
        let update = |old: Option<&[u8]>| {
            old.map(bincode::deserialize)
                .map(Result::unwrap)
                .map(func)
                .as_ref()
                .map(bincode::serialize)
                .map(Result::unwrap)
        };
        let bytes = self.update_and_fetch(key, update).unwrap()?;
        Some(bincode::deserialize(&bytes).expect("something went wrong deserializing"))
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

    let bytes = bincode::serialize(&new).expect("something went wrong deserializing");
    Some(bytes)
}

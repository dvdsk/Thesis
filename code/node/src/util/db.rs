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
    fn update<T>(&self, key: impl AsRef<[u8]>, func: fn(T) -> T) -> Option<T>
    where
        T: DeserializeOwned + Serialize;
}

impl TypedSled for sled::Tree {
    fn get_val<T: DeserializeOwned>(&self, key: impl AsRef<[u8]>) -> Option<T> {
        self.get(key)
            .unwrap()
            .map(|bytes| bincode::deserialize(&bytes).expect("something went wrong deserializing"))
    }
    fn set_val<T: Serialize>(&self, key: impl AsRef<[u8]>, val: T) {
        let bytes = bincode::serialize(&val).unwrap();
        let _ig_old_key = self.insert(key, bytes).unwrap();
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

#[cfg(test)]
mod test {
    use super::TypedSled;
    use mktemp::Temp;
    use serde::{Deserialize, Serialize};

    pub type Term = u32;
    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum Role {
        Idle,
        Clerk,
        Minister,
        President { term: Term },
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub enum Order {
        /// used as placeholder for the first entry in the log
        None,
        Assigned(Role),
        BecomePres {
            term: Term,
        },
        ResignPres,
    }

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct LogEntry {
        term: Term,
        entry: Order,
    }

    impl Default for LogEntry {
        fn default() -> Self {
            Self {
                term: 0,
                entry: Order::None,
            }
        }
    }

    #[test]
    fn read_key() {
        let temp_dir = Temp::new_dir().unwrap();
        let db = sled::open(temp_dir.join("db")).unwrap();
        let tree = db.open_tree("president").unwrap();
        let key = [2, 0, 0, 0, 0];
        tree.set_val(key, &Order::None);

        match tree.get_val([2, 0, 0, 0, 0]) {
            Some(LogEntry { term, .. }) if term == 0 => true,
            _ => false,
        };
    }
}

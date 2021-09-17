use client_protocol::PathString;

#[derive(Debug, Clone)]
pub struct Db(sled::Db);

#[derive(Debug)]
pub enum DbError {
    FileExists,
}

pub fn folder() -> sled::IVec {
    sled::IVec::default()
}

impl Db {
    fn init_change_idx(&self) {
        let key = &[0];
        let change_idx = 0u64;
        let _ = self
            .0
            .compare_and_swap(key, None as Option<&[u8]>, Some(&change_idx.to_ne_bytes()))
            .unwrap();
    }

    pub fn new() -> Self {
        let dir = Self(sled::open("db").unwrap());
        dir.init_change_idx();
        dir
    }

    pub fn get_change_idx(&self) -> u64 {
        let key = &[0];
        let vec = self.0.get(key).unwrap().unwrap();
        idx_from_ivec(vec)
    }

    pub async fn mkdir(&self, path: PathString) -> Result<(), DbError> {
        let res = self
            .0
            .compare_and_swap(&path, None as Option<&[u8]>, Some(folder()))
            .unwrap(); // crash on any internal/io db error

        if let Err(e) = res {
            if e.current.unwrap().len() > 0 {
                Err(DbError::FileExists)?;
            } // no error if dir exists
        }

        self.0.flush_async().await.unwrap();
        Ok(())
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::new();
        for (k, v) in self.0.iter().filter_map(Result::ok) {
            bytes.extend_from_slice(&k.len().to_ne_bytes());
            bytes.extend_from_slice(&k);
            bytes.extend_from_slice(&v.len().to_ne_bytes());
            bytes.extend_from_slice(&v);
        }
        bytes
    }

    pub fn replace_with_deserialized(&self, bytes: Vec<u8>) {
        use std::mem::size_of;
        self.0.clear();
        let mut i = 0;
        const len: usize = size_of::<usize>();
        while i < bytes.len() {
            let key_len = usize_from_slice(&bytes[i..len]);
            let key = &bytes[i..i+key_len];

        }
    }
}

fn usize_from_slice(slice: &[u8]) -> usize {
    let mut idx = [0u8; std::mem::size_of::<usize>()];
    idx[..].copy_from_slice(&slice);
    usize::from_ne_bytes(idx)
}

fn idx_from_ivec(vec: sled::IVec) -> u64 {
    let mut idx = [0u8; 8];
    idx[..].copy_from_slice(&vec[..]);
    u64::from_ne_bytes(idx)
}

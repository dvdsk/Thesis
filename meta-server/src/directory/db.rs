use std::convert::TryInto;

use client_protocol::{FsEntry, PathString};

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
        let db = sled::Config::new()
            .path("db")
            .mode(sled::Mode::HighThroughput)
            .open()
            .expect("check if a server is not already running");
        let dir = Self(db);
        dir.init_change_idx();
        dir
    }

    #[cfg(test)]
    pub fn new_temp() -> Self {
        let db = sled::Config::new().temporary(true).open().unwrap();
        let dir = Self(db);
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

    pub fn ls(&self, working_dir: impl Into<PathString>) -> Vec<FsEntry> {
        let working_dir = working_dir.into().into_bytes();
        let next_dir = next_dir(&working_dir);
        self.0
            .range(working_dir..next_dir)
            .filter_map(Result::ok)
            .map(into_fs_entry)
            .collect()
    }

    pub fn serialize(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(self.0.size_on_disk().unwrap() as usize);
        for (k, v) in self.0.iter().filter_map(Result::ok) {
            bytes.extend_from_slice(&k.len().to_ne_bytes());
            bytes.extend_from_slice(&k);
            bytes.extend_from_slice(&v.len().to_ne_bytes());
            bytes.extend_from_slice(&v);
        }
        bytes
    }

    pub async fn replace_with_deserialized(&self, bytes: &[u8]) {
        self.0.clear().unwrap();
        let mut i = 0;
        while i < bytes.len() {
            let val = deserialize_next(&mut i, bytes);
            let key = deserialize_next(&mut i, bytes);
            self.0.insert(key, val).unwrap();
        }
        self.0.flush_async().await.unwrap();
    }
}

fn deserialize_next<'a>(i: &mut usize, bytes: &'a [u8]) -> &'a [u8] {
    const USIZE_LEN: usize = std::mem::size_of::<usize>();
    let val_len = usize_from_slice(&bytes[*i..*i + USIZE_LEN]);
    *i += USIZE_LEN;
    let val = &bytes[*i..*i + val_len];
    *i += val_len;
    val
}

fn into_fs_entry((k, v): (sled::IVec, sled::IVec)) -> FsEntry {
    let path = PathString::from_utf8(k.to_vec()).unwrap();
    match v.len() {
        0 => FsEntry::Dir(path),
        _ => FsEntry::File(path),
    }
}

fn next_dir(dir: &Vec<u8>) -> Vec<u8> {
    let mut dir = dir.clone();
    for b in dir.iter_mut().rev() {
        let (new_b, overflow) = b.overflowing_add(1);
        *b = new_b;
        if !overflow {
            break;
        }
    }
    dir
}

fn usize_from_slice(slice: &[u8]) -> usize {
    usize::from_ne_bytes(slice.try_into().expect("incorrect length"))
}

fn idx_from_ivec(vec: sled::IVec) -> u64 {
    u64::from_ne_bytes(vec.as_ref().try_into().expect("incorrect length"))
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn filled_db(entries: &[FsEntry]) -> Db {
        let db = Db::new_temp();
        for e in entries {
            match e {
                FsEntry::Dir(p) => db.mkdir(p.to_owned()).await.unwrap(),
                _ => todo!(),
            }
        }
        db
    }

    fn test_entries(numb: usize) -> Vec<FsEntry> {
        (0..numb)
            .map(|i| FsEntry::Dir(format!("long/path/{}", i)))
            .collect()
    }

    #[tokio::test]
    async fn ls() {
        let correct = test_entries(5);
        let db = filled_db(&correct).await;
        let list = db.ls("long/path");
        for (ls_entry, correct) in list.iter().zip(correct.iter()) {
            assert_eq!(ls_entry, correct);
        }
    }

    #[tokio::test]
    async fn serialize_and_deserialize() {
        let correct = test_entries(5);
        let db = filled_db(&correct).await;
        let bytes = db.serialize();

        std::mem::drop(db);
        let db = Db::new_temp();
        db.replace_with_deserialized(&bytes).await;

        let list = db.ls("long/path/{}");
        for (ls_entry, correct) in list.iter().zip(correct.iter()) {
            assert_eq!(ls_entry, correct);
        }
    }
}

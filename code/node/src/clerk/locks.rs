use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;

use protocol::AccessKey;
use crate::directory::Directory;

#[derive(Default, Clone)]
pub struct Locks(Arc<Mutex<HashMap<AccessKey, (PathBuf, Range<u64>)>>>);

impl Locks {
    pub(crate) async fn reset(&mut self, key: AccessKey, dir: &mut Directory) {
        todo!()
    }

    pub(crate) async fn reset_all(&mut self, dir: &mut Directory) {
        let mut map = self.0.lock().await;
        for (_, (path, range)) in map.drain() {
            todo!()
        }
    }
}

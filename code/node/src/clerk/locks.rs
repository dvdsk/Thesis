use std::collections::HashMap;
use std::ops::Range;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::Mutex;
use tracing::instrument;

use crate::directory::Directory;
use protocol::AccessKey;

#[derive(Default, Clone)]
pub struct Locks(Arc<Mutex<HashMap<(PathBuf, AccessKey), AccessKey>>>);

impl Locks {
    #[instrument(skip(self, dir))]
    pub(super) async fn reset(
        &mut self,
        path: PathBuf,
        remote_key: AccessKey,
        dir: &mut Directory,
    ) {
        let mut map = self.0.lock().await;
        let local_key = map.remove(&(path.clone(), remote_key)).unwrap();
        dir.revoke_access(&path, local_key)
    }

    #[instrument(skip(self, dir))]
    pub(super) async fn reset_all(&mut self, dir: &mut Directory) {
        let mut map = self.0.lock().await;
        for ((path, _), local_key) in map.drain() {
            dir.revoke_access(&path, local_key)
        }
    }

    #[instrument(skip(self, dir))]
    pub(super) async fn add(
        &mut self,
        dir: &mut Directory,
        path: PathBuf,
        range: Range<u64>,
        remote_key: AccessKey,
    ) {
        let local_key = dir
            .get_exclusive_access(&path, &range)
            .unwrap()
            .expect("path+range is already locked");
        let mut map = self.0.lock().await;
        let prev = map.insert((path, remote_key), local_key);
        assert!(prev.is_none(), "path+range was already locked");
    }
}

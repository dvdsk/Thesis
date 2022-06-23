use std::net::SocketAddr;
use std::path::{Path, PathBuf};

use rand::prelude::SliceRandom;
use rand::thread_rng;
use tracing::{debug, instrument};

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ministry {
    pub staff: protocol::Staff,
    pub subtree: PathBuf,
}

#[derive(Default, Debug)]
pub(super) struct Map(Vec<Ministry>);

impl Map {
    // might be optimized with Trie as datastrucuture IF AND ONLY IF this
    // is a bottleneck
    fn staff_for(&self, path: &Path) -> Option<&protocol::Staff> {
        for ministry in self.0.iter().rev() {
            if path.starts_with(&ministry.subtree) {
                return Some(&ministry.staff);
            }
        }
        None
    }

    fn staff_for_mut(&mut self, path: &Path) -> Option<&mut protocol::Staff> {
        for ministry in self.0.iter_mut().rev() {
            if path.starts_with(&ministry.subtree) {
                return Some(&mut ministry.staff);
            }
        }
        None
    }

    #[instrument(skip(self), ret)]
    pub fn clerk_for(&self, path: &Path) -> Option<SocketAddr> {
        let mut rng = thread_rng();
        self.staff_for(path)
            .map(|s| s.clerks.choose(&mut rng))
            .flatten()
            .cloned()
    }

    #[instrument(skip(self), ret)]
    pub fn minister_for(&self, path: &Path) -> Option<SocketAddr> {
        self.staff_for(path).map(|s| s.minister).flatten()
    }

    #[instrument(skip(self))]
    pub fn invalidate(&mut self, path: &Path, addr: SocketAddr) {
        let staff = match self.staff_for_mut(path) {
            None => return,
            Some(staff) => staff,
        };

        if staff.minister == Some(addr) {
            staff.minister = None;
            return;
        }

        if let Some((idx, _)) = staff.clerks.iter().enumerate().find(|(_, a)| *a == &addr) {
            staff.clerks.remove(idx);
        }
    }

    #[instrument(skip(self))]
    pub fn insert(&mut self, ministry: Ministry) {
        let res = self
            .0
            .binary_search_by_key(&ministry.subtree, |m| m.subtree.clone());
        match res {
            Ok(idx) => {
                debug!("updating existing map entry");
                self.0.insert(idx, ministry);
            }
            Err(idx) => self.0.insert(idx, ministry),
        };
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use protocol::Staff;

    fn test_ministry(path: &'static str) -> Ministry {
        Ministry {
            staff: Staff {
                minister: Some("0.0.0.0:80".parse().unwrap()),
                clerks: Vec::new(),
            },
            subtree: PathBuf::from(path),
        }
    }

    #[test]
    fn test_map_lookup() {
        let mut map = Map::default();
        map.insert(test_ministry("/testA/testB"));
        map.insert(test_ministry("/testC"));
        map.insert(test_ministry("/"));
        map.insert(test_ministry("/testA"));

        assert_eq!(
            map.staff_for(&PathBuf::from("/")).unwrap(),
            &test_ministry("/").staff
        );
        assert_eq!(
            map.staff_for(&PathBuf::from("/testD")).unwrap(),
            &test_ministry("/").staff
        );
        assert_eq!(
            map.staff_for(&PathBuf::from("/testA/testB/hello")).unwrap(),
            &test_ministry("/testA/testB").staff
        );
        assert_eq!(
            map.staff_for(&PathBuf::from("/testA/teetB/hello")).unwrap(),
            &test_ministry("/testA").staff
        );
    }
}

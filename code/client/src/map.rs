use std::net::SocketAddr;
use std::path::{Path, PathBuf};

#[derive(Default, Clone, Debug, PartialEq, Eq)]
pub(crate) struct Ministry {
    pub minister: Option<SocketAddr>,
    pub clerk: Option<SocketAddr>,
    pub subtree: PathBuf,
}

impl Ministry {
    fn update(&mut self, other: Self) {
        self.minister = self.minister.and(other.minister);
        self.clerk = self.clerk.and(other.clerk);
    }
}

#[derive(Default, Debug)]
pub(super) struct Map(Vec<Ministry>);

impl Map {
    // might be optimized with Trie as datastrucuture IF AND ONLY IF this
    // is a bottleneck
    pub fn ministry_for(&self, path: &Path) -> Option<&Ministry> {
        for ministry in self.0.iter().rev() {
            if path.starts_with(&ministry.subtree) {
                return Some(ministry);
            }
        }

        None
    }

    pub fn insert(&mut self, ministry: Ministry) {
        let res = self
            .0
            .binary_search_by_key(&ministry.subtree, |m| m.subtree.clone());
        match res {
            Ok(idx) => {
                let existing = self.0.get_mut(idx).unwrap();
                existing.update(ministry);
            }
            Err(idx) => self.0.insert(idx, ministry),
        };
    }
}

#[cfg(test)]
mod tests {
    use std::str::FromStr;

    use super::*;

    fn test_ministry(path: &'static str) -> Ministry {
        Ministry {
            minister: Some(SocketAddr::from_str(&format!("0.0.0.0:80")).unwrap()),
            clerk: None,
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
            map.ministry_for(&PathBuf::from("/")).unwrap(),
            &test_ministry("/")
        );
        assert_eq!(
            map.ministry_for(&PathBuf::from("/testD")).unwrap(),
            &test_ministry("/")
        );
        assert_eq!(
            map.ministry_for(&PathBuf::from("/testA/testB/hello"))
                .unwrap(),
            &test_ministry("/testA/testB")
        );
        assert_eq!(
            map.ministry_for(&PathBuf::from("/testA/teetB/hello"))
                .unwrap(),
            &test_ministry("/testA")
        );
    }
}

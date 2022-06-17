use std::ops::Range;

use color_eyre::Result;
use serde::{Deserialize, Serialize};
use slotmap::SlotMap;

/// access is explicitly revoked once
/// a client times out
#[derive(Serialize, Deserialize, Debug)]
enum Access {
    Reader(Range<u64>),
    Writer(Range<u64>),
}

impl Access {
    fn range(&self) -> &Range<u64> {
        match self {
            Self::Reader(r) => r,
            Self::Writer(r) => r,
        }
    }
}

slotmap::new_key_type! { pub struct AccessKey; }

#[derive(Default, Serialize, Deserialize)]
pub struct Entry {
    size: usize,
    /// list sorted by start position
    areas: SlotMap<AccessKey, Access>,
}

impl Entry {
    pub fn from_bytes(bytes: &[u8]) -> Self {
        bincode::deserialize(bytes).unwrap()
    }

    pub fn to_bytes(&self) -> Vec<u8> {
        bincode::serialize(self).unwrap()
    }

    pub fn overlapping(&self, req: &Range<u64>) -> Vec<AccessKey> {
        use crate::util::Overlap;
        self.areas
            .iter()
            .filter(|(_, access)| access.range().has_overlap_with(req))
            .map(|(key, _)| key)
            .collect()
    }

    pub fn overlapping_write_access(&self, req: &Range<u64>) -> bool {
        use crate::util::Overlap;
        for access in self.areas.values() {
            match access {
                Access::Writer(range) if range.has_overlap_with(req) => return true,
                _ => (),
            }
        }
        return false;
    }

    pub fn add_write_access(&mut self, range: &Range<u64>) -> AccessKey {
        self.areas.insert(Access::Writer(range.clone()))
    }

    pub fn add_read_access(&mut self, range: &Range<u64>) -> AccessKey {
        self.areas.insert(Access::Reader(range.clone()))
    }

    pub fn revoke_access(&mut self, key: AccessKey) {
        self.areas.remove(key);
    }
}

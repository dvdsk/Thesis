use serde::{Deserialize, Serialize};

pub mod connection;

pub trait Message<'de>: Serialize + Deserialize<'de> {
    fn from_buf(buf: &'de [u8]) -> Self {
        bincode::deserialize(buf).unwrap()
    }
    fn serialize_into(&self, buf: &mut [u8]) -> usize {
        bincode::serialize_into(buf, self).expect("could not serialize");
        todo!()
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum Request {
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Response {
}

// #[cfg(test)]
// mod tests {
//     use super::*;

//     #[test]
//     fn serialized_request_size() {
//         use Request::*;

//         let test_cases = vec![
//             (20, OpenReadOnly("test".into(), Existence::Needed)),
//             (25, OpenReadOnly("test/test".into(), Existence::Allowed)),
//         ];

//         for (size, obj) in test_cases {
//             let v: Vec<u8> = bincode::serialize(&obj).unwrap();
//             assert_eq!(size, v.len())
//         }
//     }
// }

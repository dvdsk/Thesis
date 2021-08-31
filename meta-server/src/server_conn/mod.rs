pub mod protocol;
pub mod to_readserv;
pub mod to_writeserv;
pub mod discovery;
pub mod election;

pub const MDNS_NAME: &'static str = "_write_meta_server";

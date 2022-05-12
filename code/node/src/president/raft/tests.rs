use super::*;
mod util;
mod mock;
mod election;
mod consensus;
mod append_request;

const TIMEOUT: Duration = Duration::from_millis(500);


use super::*;
mod util;
mod mock;
mod election;
mod consensus;
mod append_request;

const TEST_TIMEOUT: Duration = Duration::from_millis(2_000);


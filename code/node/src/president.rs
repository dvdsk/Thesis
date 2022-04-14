use multicast_discovery::Chart as mChart;

mod raft;
pub use raft::Log;

type Chart = mChart<2, u16>;
pub(crate) fn work(chart: &mut Chart) -> crate::Role {
    todo!()
}

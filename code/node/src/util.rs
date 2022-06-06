use color_eyre::eyre::{Result, WrapErr};
use result_tools::*;

use std::fs;
use std::io;
use std::net::IpAddr;
use std::net::Ipv4Addr;
use std::net::SocketAddr;
use std::num::NonZeroU16;
use std::path::Path;
use std::path::PathBuf;
use tokio::net::TcpListener;

mod db;
mod logging;
pub use db::TypedSled;
pub use logging::setup_errors;
#[allow(unused_imports)] // used by unit tests
pub(crate) use logging::setup_test_tracing;
pub use logging::setup_tracing;

#[cfg(test)]
#[allow(dead_code)]
pub fn free_udp_port() -> Result<(std::net::UdpSocket, u16)> {
    use socket2::{Domain, SockAddr, Socket, Type};

    let socket = Socket::new(Domain::IPV4, Type::DGRAM, None)?;
    socket.set_reuse_port(true)?;

    let ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let addr = SocketAddr::new(ip, 0);
    let addr = SockAddr::from(addr);
    socket.bind(&addr).unwrap();

    let open_port = socket.local_addr().unwrap().as_socket().unwrap().port();
    let socket = std::net::UdpSocket::from(socket);
    Ok((socket, open_port))
}

#[allow(dead_code)]
pub async fn open_socket(port: Option<NonZeroU16>) -> Result<(TcpListener, u16)> {
    let ip = IpAddr::V4(Ipv4Addr::UNSPECIFIED);
    let addr = SocketAddr::new(ip, port.map(NonZeroU16::get).unwrap_or(0));
    let listener = TcpListener::bind(addr)
        .await
        .wrap_err("Could not bind to address: {addr}")?;

    let open_port = listener.local_addr().unwrap().port();
    match port {
        None => tracing::trace!("OS assigned free TCP port: {open_port}"),
        Some(p) => tracing::trace!("opend TCP port: {p}"),
    }
    Ok((listener, open_port))
}

#[allow(dead_code)]
pub fn runtime_dir() -> PathBuf {
    use std::io::ErrorKind::AlreadyExists;
    let temp = std::env::temp_dir().join("thesis");
    fs::create_dir(&temp)
        .to_ok_if(|e| e.kind() == AlreadyExists)
        .unwrap();
    temp
}

#[allow(dead_code)]
pub fn run_number(dir: &Path) -> u16 {
    let path = dir.join("run.txt");
    let run = match fs::read_to_string(&path).map_err(|e| e.kind()) {
        Err(io::ErrorKind::NotFound) => 0,
        Err(e) => panic!("could not access run numb file: {e:?}"),
        Ok(run) => run.parse().unwrap(),
    };
    fs::write(path, (run + 1).to_string().as_bytes()).unwrap();
    run
}

#[allow(dead_code)]
pub fn div_ceil(numerator: usize, denominator: usize) -> usize {
    (numerator + denominator - 1) / denominator
}

use core::future::Future;
use core::pin::Pin;
use core::task::{Context, Poll};
use tokio::task;

/// Spawn a new tokio Task and cancel it on drop.
#[allow(dead_code)]
#[track_caller]
pub fn spawn<T>(future: T) -> Wrapper<T::Output>
where
    T: Future + Send + 'static,
    T::Output: Send + 'static,
{
    Wrapper(task::spawn(future))
}

/// Cancels the wrapped tokio Task on Drop.
pub struct Wrapper<T>(task::JoinHandle<T>);

impl<T> Future for Wrapper<T> {
    type Output = Result<T, task::JoinError>;
    fn poll(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Self::Output> {
        unsafe { Pin::new_unchecked(&mut self.0) }.poll(cx)
    }
}

impl<T> Drop for Wrapper<T> {
    fn drop(&mut self) {
        // do `let _ = self.0.cancel()` for `async_std::task::Task`
        self.0.abort();
    }
}

#[cfg(test)]
mod test {
    use super::*;
    mod run_number {
        use mktemp::Temp;

        use super::*;
        #[test]
        fn increases() {
            let dir = Temp::new_dir().unwrap();
            for correct in 0..10 {
                let run_numb = run_number(&dir);
                assert_eq!(run_numb, correct);
            }
        }
    }
}

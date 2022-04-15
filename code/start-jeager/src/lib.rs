use std::ffi::OsStr;
use std::fs;
use std::io::Read;
use std::path::Path;

use flate2::read::GzDecoder;
use tar::Archive;
use tracing::info;

const NAME: &str = "jaeger-all-in-one";
async fn download(dir: impl AsRef<Path>) {
    const URL: &str = "https://github.com/jaegertracing/jaeger/releases/download/v1.33.0/jaeger-1.33.0-linux-amd64.tar.gz";
    fn all_in_one_bin<R: std::io::Read>(e: &tar::Entry<R>) -> bool {
        match e.path() {
            Ok(p) => Some(NAME) == p.file_name().and_then(OsStr::to_str),
            _ => false,
        }
    }

    info!("downloading jeager");
    let bytes = reqwest::get(URL).await.unwrap().bytes().await.unwrap();
    info!("unpacking jeager");
    let mut unpacked = Vec::new();
    let tar = GzDecoder::new(&bytes[..]);
    Archive::new(tar)
        .entries()
        .unwrap()
        .filter_map(Result::ok)
        .find(all_in_one_bin)
        .unwrap()
        .read_to_end(&mut unpacked)
        .unwrap();
    fs::write(dir.as_ref().join(NAME), unpacked).unwrap();
}

fn already_running(name: &str) -> bool {
    use psutil::process::processes;
    processes()
        .unwrap()
        .into_iter()
        .filter_map(Result::ok)
        .map(|p| p.name())
        .filter_map(Result::ok)
        .any(|n| n == name)
}

pub async fn start_if_not_running(dir: &Path) {
    if already_running(NAME) {
        info!("jeager already running not starting new instance");
        return;
    }

    let path = dir.join(NAME);
    if !path.is_file() {
        download(dir).await;
    }

    tokio::process::Command::new(path)
        .arg("--query.http-server.host-port")
        .arg("16686")
        .kill_on_drop(false)
        .spawn()
        .unwrap()
        .wait()
        .await
        .unwrap();
}

#[cfg(test)]
mod tests {
    use std::os::unix::prelude::MetadataExt;

    use super::*;
    use mktemp::Temp;
    use more_asserts::assert_gt;

    #[tokio::test]
    async fn download_jeager() {
        let dir = Temp::new_dir().unwrap();
        download(&dir).await;
        let file = dir.as_path().join(NAME);
        assert!(file.exists());
        let size = file.metadata().unwrap().size();
        assert_gt!(size, 40_000_000);
    }

    #[tokio::test]
    async fn detect_running() {
        use psutil::process::Process;
        let current = Process::current().unwrap().name().unwrap();
        assert_eq!(already_running(&current), true);
        assert_eq!(already_running("name_of_a_not_running_process"), false);
    }
}

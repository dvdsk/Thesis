use color_eyre::eyre::{eyre, ContextCompat, Result, WrapErr};
use std::{path::PathBuf, str::FromStr};

mod bench;

type Client = client::Client<client::ChartNodes<3, 2>>;

pub async fn client(client: &mut Client, buffer: String) -> Result<()> {
    let mut args = buffer.split(' ');
    let cmd = args.next();
    let path = args
        .next()
        .map(str::trim)
        .map(PathBuf::from)
        .wrap_err("each argument needs a path")?;
    let bytes = args
        .next()
        .map(str::trim)
        .map(|s| {
            let s = s.replace('_', "");
            usize::from_str(&s)
        })
        .map(|r| r.wrap_err("bytes needs to be an unsigned int"));

    match cmd {
        Some("ls") => println!("list: {:?}", client.list(path).await),
        Some("touch") => client.create_file(path).await,
        Some("read") => {
            let mut file = client.open_readable(path).await;
            let bytes = bytes.ok_or_else(|| eyre!("read needs bytes as third arg"))??;
            // let mut data = vec![0u8; bytes];
            file.read(&mut vec![0u8; bytes]).await;
        }
        Some("write") => {
            let mut file = client.open_writeable(path).await;
            let bytes = bytes.ok_or_else(|| eyre!("read needs bytes as third arg"))??;
            file.write(&vec![42u8; bytes]).await;
        }
        _ => panic!("invalid command: {cmd:?}"),
    }
    Ok(())
}

pub async fn bench(client: &mut Client, buffer: String) -> Result<()> {
    let mut args = buffer.split(' ');
    let cmd = args.next();
    let mut next_arg = || {
        args.next()
            .map(str::trim)
            .map(|s| {
                let s = s.replace('_', "");
                usize::from_str(&s)
            })
            .map(|r| r.wrap_err("argument should be a signed interger"))
    };
    match cmd {
        Some("bench_leases") | Some("bl_") => {
            let readers = next_arg().ok_or_else(|| eyre!("first arg should be #readers"))??;
            let writers = next_arg().ok_or_else(|| eyre!("second arg should be #writers"))??;
            bench::leases(client, readers, writers, 2).await;
        }
        Some("bench_meta") | Some("bm_") => {
            let creators = next_arg().ok_or_else(|| eyre!("first arg should be #creators"))??;
            let listers = next_arg().ok_or_else(|| eyre!("second arg should be #listers"))??;
            bench::meta(creators, listers, 2).await;
        }
        Some("colliding_writers") | Some("cw_") => {
            let writers = next_arg().ok_or_else(|| eyre!("first arg should be #writers"))??;
            let row_len = next_arg().ok_or_else(|| eyre!("second arg should be #row len"))??;
            bench::colliding(client, writers, row_len as u64).await;
        }
        _ => panic!("invalid command: {cmd:?}"),
    }

    Ok(())
}

use tokio::net::TcpStream;
use tokio_serde::formats::Bincode;
use tokio_util::codec::{Framed, LengthDelimitedCodec};

type FrameType = Framed<TcpStream, LengthDelimitedCodec>;
type Codec<I, O> = Bincode<I, O>;

/// A stream recieving: I: incoming item, O: outgoing
pub type MsgStream<I, O> = tokio_serde::Framed<FrameType, I, O, Codec<I, O>>;

/// I: incoming item, O: outgoing
pub fn wrap<I, O>(stream: TcpStream) -> MsgStream<I, O> {
    let length_delimited = Framed::new(stream, LengthDelimitedCodec::new());
    tokio_serde::Framed::new(length_delimited, Bincode::<I, O>::default())
}

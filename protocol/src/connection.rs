use tokio::net::TcpStream;
use tokio_serde::formats::{Bincode, SymmetricalBincode};
use tokio_util::codec::{FramedRead, LengthDelimitedCodec};

type ReadFrame = FramedRead<tokio::net::TcpStream, LengthDelimitedCodec>;
type Codec<I,S> = Bincode<I, S>;
pub type ReadStream<I, S> = tokio_serde::Framed<ReadFrame, I, S, Codec<I,S>>;

// I is the item being recieved, S the item being send
pub fn wrap<I, S>(socket: TcpStream) -> ReadStream<I, S> {
    let length_delimited = FramedRead::new(socket, LengthDelimitedCodec::new());

    let deserialized_stream = tokio_serde::Framed::new(
        length_delimited,
        Bincode::<I,S>::default(),
    );

    deserialized_stream
}

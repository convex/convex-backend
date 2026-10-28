#![feature(coroutines)]
#![feature(try_blocks)]
#![feature(iter_from_coroutine)]
#![feature(iterator_try_collect)]

use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;
use futures::{
    Stream,
    TryStreamExt,
};

mod metrics;
pub mod storage;
mod types;

/// For reasons unknown, the AWS SDK folks have decided that the public APIs
/// can't contain any third-party types, so the `ByteStream` no longer
/// implements `Stream`.
/// We have to yoink this adapter trait from one of their internal crates to
/// re-add the implementation.
pub trait ByteStreamCompat {
    fn into_stream(self) -> impl Stream<Item = Result<Bytes, std::io::Error>>;
}

impl ByteStreamCompat for ByteStream {
    fn into_stream(self) -> impl Stream<Item = Result<Bytes, std::io::Error>> {
        aws_smithy_http::futures_stream_adapter::FuturesStreamCompatByteStream::new(self)
            .map_err(Into::into)
    }
}

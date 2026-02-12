use crate::{ZlibDecompressionError, ZlibStreamDecompressor};
use futures_core::Stream;
use std::pin::Pin;
use std::task::{Context, Poll};

#[cfg(feature = "tokio-runtime")]
const TOKIO_BLOCK_IN_PLACE_THRESHOLD: usize = 64 * 1024;

pub struct ZlibStream<V: AsRef<[u8]>, T: Stream<Item = V> + Unpin> {
    decompressor: ZlibStreamDecompressor,
    stream: T,
}

impl<V: AsRef<[u8]>, T: Stream<Item = V> + Unpin> ZlibStream<V, T> {
    /// Creates a new ZlibStream object with the default decompressor and the underlying
    /// stream as data source
    pub fn new(stream: T) -> Self {
        Self {
            decompressor: Default::default(),
            stream,
        }
    }

    /// Creates a new ZlibStream object with the specified decompressor and the underlying
    /// stream as data source
    pub fn new_with_decompressor(decompressor: ZlibStreamDecompressor, stream: T) -> Self {
        Self {
            decompressor,
            stream,
        }
    }
}

impl<V: AsRef<[u8]>, T: Stream<Item = V> + Unpin> Stream for ZlibStream<V, T> {
    type Item = Result<Vec<u8>, ZlibDecompressionError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        loop {
            match Pin::new(&mut self.stream).poll_next(cx) {
                Poll::Ready(Some(frame)) => {
                    #[cfg(feature = "tokio-runtime")]
                    let result = {
                        if frame.as_ref().len() < TOKIO_BLOCK_IN_PLACE_THRESHOLD {
                            self.decompressor.decompress(frame)
                        } else {
                            tokio::task::block_in_place(|| self.decompressor.decompress(frame))
                        }
                    };

                    #[cfg(not(feature = "tokio-runtime"))]
                    let result = self.decompressor.decompress(frame);

                    match result {
                        Ok(data) => return Poll::Ready(Some(Ok(data))),
                        Err(ZlibDecompressionError::NeedMoreData) => continue,
                        Err(err) => return Poll::Ready(Some(Err(err))),
                    }
                }
                Poll::Ready(None) => return Poll::Ready(None),
                Poll::Pending => return Poll::Pending,
            }
        }
    }
}

#[cfg(feature = "bytes-api")]
pub struct ZlibBytesStream<V: AsRef<[u8]>, T: Stream<Item = V> + Unpin> {
    inner: ZlibStream<V, T>,
}

#[cfg(feature = "bytes-api")]
impl<V: AsRef<[u8]>, T: Stream<Item = V> + Unpin> ZlibBytesStream<V, T> {
    pub fn new(stream: T) -> Self {
        Self {
            inner: ZlibStream::new(stream),
        }
    }

    pub fn new_with_decompressor(decompressor: ZlibStreamDecompressor, stream: T) -> Self {
        Self {
            inner: ZlibStream::new_with_decompressor(decompressor, stream),
        }
    }
}

#[cfg(feature = "bytes-api")]
impl<V: AsRef<[u8]>, T: Stream<Item = V> + Unpin> Stream for ZlibBytesStream<V, T> {
    type Item = Result<bytes::Bytes, ZlibDecompressionError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.inner).poll_next(cx) {
            Poll::Ready(Some(Ok(data))) => Poll::Ready(Some(Ok(bytes::Bytes::from(data)))),
            Poll::Ready(Some(Err(err))) => Poll::Ready(Some(Err(err))),
            Poll::Ready(None) => Poll::Ready(None),
            Poll::Pending => Poll::Pending,
        }
    }
}

use crate::{ZlibDecompressionError, ZlibStreamDecompressor};
use flate2::DecompressError;
use futures_util::{Stream, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct ZlibStream<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> {
    decompressor: ZlibStreamDecompressor,
    stream: T,
}

impl<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> ZlibStream<V, T> {
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

impl<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> Stream for ZlibStream<V, T> {
    type Item = Result<Vec<u8>, DecompressError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        match Pin::new(&mut self.stream.next()).poll(cx) {
            Poll::Ready(vec) => {
                if let Some(vec) = vec {
                    #[cfg(feature = "tokio-runtime")]
                    let result = tokio::task::block_in_place(|| self.decompressor.decompress(vec));

                    #[cfg(not(feature = "tokio-runtime"))]
                    let result = self.decompressor.decompress(vec);

                    match result {
                        Ok(data) => Poll::Ready(Some(Ok(data))),
                        Err(ZlibDecompressionError::NeedMoreData) => {
                            cx.waker().wake_by_ref();
                            Poll::Pending
                        }
                        Err(ZlibDecompressionError::DecompressError(err)) => {
                            Poll::Ready(Some(Err(err)))
                        }
                    }
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

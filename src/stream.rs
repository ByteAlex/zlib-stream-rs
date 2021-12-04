use crate::{ZlibDecompressionError, ZlibStreamDecompressor};
use flate2::DecompressError;
use futures_util::{Stream, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};
#[cfg(feature = "thread")]
use crate::thread::ZlibStreamDecompressorThread;

pub struct ZlibStream<V: AsRef<[u8]> + Sized, T: Stream<Item=V> + Unpin> {
    #[cfg(not(feature = "thread"))]
    decompressor: ZlibStreamDecompressor,
    #[cfg(feature = "thread")]
    decompressor: ZlibStreamDecompressorThread,
    stream: T,
    #[cfg(feature = "thread")]
    thread_poll: Option<futures::channel::oneshot::Receiver<Result<Vec<u8>, ZlibDecompressionError>>>,
}

impl<V: AsRef<[u8]> + Sized, T: Stream<Item=V> + Unpin> ZlibStream<V, T> {
    /// Creates a new ZlibStream object with the default decompressor and the underlying
    /// stream as data source
    pub fn new(stream: T) -> Self {
        #[cfg(not(feature = "thread"))]
            let decompressor = Default::default();
        #[cfg(feature = "thread")]
            let decompressor = ZlibStreamDecompressorThread::spawn(Default::default());
        Self {
            decompressor,
            stream,
            #[cfg(feature = "thread")]
            thread_poll: None,
        }
    }

    /// Creates a new ZlibStream object with the specified decompressor and the underlying
    /// stream as data source
    pub fn new_with_decompressor(decompressor: ZlibStreamDecompressor, stream: T) -> Self {
        #[cfg(feature = "thread")]
            let decompressor = ZlibStreamDecompressorThread::spawn(decompressor);
        Self {
            decompressor,
            stream,
            #[cfg(feature = "thread")]
            thread_poll: None,
        }
    }
}

impl<V: AsRef<[u8]> + Sized, T: Stream<Item=V> + Unpin> Stream for ZlibStream<V, T> {
    type Item = Result<Vec<u8>, DecompressError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        #[cfg(feature = "thread")]
        if let Some(poll) = &mut self.thread_poll {
            match Pin::new(poll).poll(cx) {
                Poll::Ready(outer) => {
                    match outer {
                        Ok(result) => {
                            match result {
                                Ok(data) => return Poll::Ready(Some(Ok(data))),
                                Err(ZlibDecompressionError::DecompressError(err)) => return Poll::Ready(Some(Err(err))),
                                _ => {}
                            }
                        }
                        Err(_cancelled) => return Poll::Ready(None)
                    }
                }
                Poll::Pending => {
                    cx.waker().wake_by_ref();
                    return Poll::Pending
                },
            }
        }
        match Pin::new(&mut self.stream.next()).poll(cx) {
            Poll::Ready(vec) => {
                if let Some(vec) = vec {
                    let result = self.decompressor.decompress(vec.as_ref().to_vec());

                    #[cfg(not(feature = "thread"))]
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


                    #[cfg(feature = "thread")]
                        {
                            self.thread_poll = Some(result);
                            Poll::Pending
                        }
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

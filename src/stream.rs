#[cfg(feature = "thread")]
use crate::thread::ZlibStreamDecompressorThread;
use crate::{ZlibDecompressionError, ZlibStreamDecompressor};
use flate2::DecompressError;
use futures_util::{Stream, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct ZlibStream<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> {
    #[cfg(not(feature = "thread"))]
    decompressor: ZlibStreamDecompressor,
    #[cfg(feature = "thread")]
    decompressor: ZlibStreamDecompressorThread,
    stream: T,
    #[cfg(feature = "thread")]
    thread_poll:
        Option<futures::channel::oneshot::Receiver<Result<Vec<u8>, ZlibDecompressionError>>>,
}

impl<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> ZlibStream<V, T> {
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

impl<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> Stream for ZlibStream<V, T> {
    type Item = Result<Vec<u8>, DecompressError>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        #[cfg(feature = "thread")]
        if let Some(poll) = &mut self.thread_poll {
            if let Some(poll) = poll_decompress_channel(poll, cx) {
                match poll {
                    Poll::Ready(_) => {
                        self.thread_poll = None;
                    }
                    Poll::Pending => {
                        cx.waker().wake_by_ref();
                    }
                }
                return poll;
            }
        }
        match Pin::new(&mut self.stream.next()).poll(cx) {
            Poll::Ready(vec) => {
                if let Some(vec) = vec {
                    #[cfg(not(feature = "thread"))]
                    let result = self.decompressor.decompress(vec);
                    #[cfg(feature = "thread")]
                    let mut result = self.decompressor.decompress(vec.as_ref().to_vec());

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
                        if let Some(poll) = poll_decompress_channel(&mut result, cx) {
                            if let Poll::Pending = poll {
                                self.thread_poll = Some(result);
                                cx.waker().wake_by_ref();
                            }
                            poll
                        } else {
                            cx.waker().wake_by_ref();
                            Poll::Pending
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

#[cfg(feature = "thread")]
fn poll_decompress_channel(
    channel: &mut futures::channel::oneshot::Receiver<Result<Vec<u8>, ZlibDecompressionError>>,
    cx: &mut Context<'_>,
) -> Option<Poll<Option<Result<Vec<u8>, DecompressError>>>> {
    match Pin::new(channel).poll(cx) {
        Poll::Ready(outer) => match outer {
            Ok(result) => match result {
                Ok(data) => Some(Poll::Ready(Some(Ok(data)))),
                Err(ZlibDecompressionError::DecompressError(err)) => {
                    Some(Poll::Ready(Some(Err(err))))
                }
                _ => None,
            },
            Err(_cancelled) => return Some(Poll::Ready(None)),
        },
        Poll::Pending => {
            return Some(Poll::Pending);
        }
    }
}

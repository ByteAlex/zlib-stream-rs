use futures_util::{Stream, StreamExt};
use std::future::Future;
use std::pin::Pin;
use std::task::{Context, Poll};

pub struct ChunkedByteStream<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> {
    stream: T,
    max_chunk_size: usize,
    pending_frame: Option<Vec<u8>>,
}

impl<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> ChunkedByteStream<V, T> {
    /// Creates a new ChunkedByteStream object with a defined chunk size which splits incoming
    /// vec chunks into chunks of the defined `max_chunk_size`.
    /// `max_chunk_size` must not be lower than 64 to avoid issues with the `ZlibStream` implementation.
    pub fn new(stream: T, max_chunk_size: usize) -> Self {
        Self {
            stream,
            max_chunk_size,
            pending_frame: None,
        }
    }
}

impl<V: AsRef<[u8]> + Sized, T: Stream<Item = V> + Unpin> Stream for ChunkedByteStream<V, T> {
    type Item = Vec<u8>;

    fn poll_next(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<Option<Self::Item>> {
        if let Some(frame) = &self.pending_frame {
            let frame = frame.clone();
            let send_frame = if frame.len() > self.max_chunk_size {
                self.pending_frame = Some(frame[self.max_chunk_size..].to_owned());
                frame[..self.max_chunk_size].to_owned()
            } else {
                self.pending_frame = None;
                frame
            };
            return Poll::Ready(Some(send_frame));
        }
        match Pin::new(&mut self.stream.next()).poll(cx) {
            Poll::Ready(data) => {
                if let Some(data) = data {
                    let vec = data.as_ref().to_vec();
                    if vec.len() > self.max_chunk_size {
                        self.pending_frame = Some(vec[self.max_chunk_size..].to_owned());
                        Poll::Ready(Some(vec[..self.max_chunk_size].to_owned()))
                    } else {
                        Poll::Ready(Some(vec))
                    }
                } else {
                    Poll::Ready(None)
                }
            }
            Poll::Pending => Poll::Pending,
        }
    }
}

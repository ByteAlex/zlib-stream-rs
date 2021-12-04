use crate::{ZlibDecompressionError, ZlibStreamDecompressor};
pub use futures::channel::oneshot::Canceled;
use std::sync::mpsc::{sync_channel, SyncReceiver, SyncSender};

#[derive(Clone)]
pub struct ZlibStreamDecompressorThread {
    sender: SyncSender<ThreadMessage>,
}

enum ThreadMessage {
    Finish,
    Decompress(
        Vec<u8>,
        futures::channel::oneshot::Sender<Result<Vec<u8>, ZlibDecompressionError>>,
    ),
}

impl ZlibStreamDecompressorThread {
    pub fn spawn(decompressor: ZlibStreamDecompressor) -> ZlibStreamDecompressorThread {
        let (tx, rx) = sync_channel(128);
        #[cfg(not(feature = "tokio-runtime"))]
        std::thread::spawn(move || ZlibStreamDecompressorThread::work(decompressor, rx));
        #[cfg(feature = "tokio-runtime")]
        tokio::task::spawn_blocking(move || ZlibStreamDecompressorThread::work(decompressor, rx));
        ZlibStreamDecompressorThread { sender: tx }
    }

    fn work(mut decompressor: ZlibStreamDecompressor, rx: SyncReceiver<ThreadMessage>) {
        loop {
            match rx.recv() {
                Ok(message) => {
                    match message {
                        ThreadMessage::Finish => break,
                        ThreadMessage::Decompress(data, channel) => {
                            let result = decompressor.decompress(data);
                            channel.send(result).ok(); // this is fine
                        }
                    }
                }
                Err(err) => {
                    log::error!("Decompressor Thread errored on channel recv: {}", err)
                }
            }
        }
    }

    pub fn abort(&self) {
        self.sender.send(ThreadMessage::Finish).ok();
    }

    pub fn decompress(
        &self,
        data: Vec<u8>,
    ) -> futures::channel::oneshot::Receiver<Result<Vec<u8>, ZlibDecompressionError>> {
        let (tx, rx) = futures::channel::oneshot::channel();
        self.sender.send(ThreadMessage::Decompress(data, tx)).ok();
        rx
    }
}

impl Drop for ZlibStreamDecompressorThread {
    fn drop(&mut self) {
        self.abort();
    }
}

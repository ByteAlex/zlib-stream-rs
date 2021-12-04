use crate::{ZlibStreamDecompressor, ZlibDecompressionError};
use std::sync::mpsc::{Sender, channel};

#[derive(Clone)]
pub struct ZlibStreamDecompressorThread {
    sender: Sender<ThreadMessage>,
}

enum ThreadMessage {
    Finish,
    Decompress(Vec<u8>, futures::channel::oneshot::Sender<Result<Vec<u8>, ZlibDecompressionError>>)
}

impl ZlibStreamDecompressorThread {


    pub fn spawn(decompressor: ZlibStreamDecompressor) -> ZlibStreamDecompressorThread {
        let (tx, rx) = channel();
        std::thread::spawn(move || {
            let mut decompressor = decompressor;
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
        });
        ZlibStreamDecompressorThread {
            sender: tx,
        }
    }

    pub fn abort(&self) {
        self.sender.send(ThreadMessage::Finish).ok();
    }

    pub fn decompress(&self, data: Vec<u8>) -> futures::channel::oneshot::Receiver<Result<Vec<u8>, ZlibDecompressionError>> {
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
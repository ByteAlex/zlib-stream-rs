use flate2::DecompressError;
use flate2::{Decompress, FlushDecompress, Status};
use thiserror::Error;

#[derive(Error, Debug)]
pub enum ZlibDecompressionError {
    #[error("An error occurred when trying to decompress the input: {0}")]
    DecompressError(#[from] DecompressError),
    #[error("Stream is not completed; Waiting for more data...")]
    NeedMoreData,
}

const ZLIB_END_BUF: [u8; 4] = [0, 0, 255, 255];

pub struct ZlibStreamDecompressor {
    inflate: Decompress,
    read_buf: Vec<u8>,
}

impl ZlibStreamDecompressor {
    pub fn new() -> Self {
        Self {
            inflate: Decompress::new(true),
            read_buf: Vec::new(),
        }
    }

    pub async fn decompress(
        &mut self,
        mut frame: Vec<u8>,
    ) -> Result<Vec<u8>, ZlibDecompressionError> {
        if self.read_buf.is_empty() {
            self.read_buf = frame;
        } else {
            self.read_buf.append(&mut frame);
        }
        if !self.read_buf.ends_with(&ZLIB_END_BUF) {
            return Err(ZlibDecompressionError::NeedMoreData);
        }
        let size_in = self.read_buf.len();
        let mut read_offset = 0usize;
        let mut out = Vec::with_capacity(self.read_buf.len() * 2);
        let mut output_buf = vec![];
        loop {
            let bytes_before = self.inflate.total_in();
            let status = self.inflate.decompress_vec(
                &self.read_buf[read_offset..],
                &mut out,
                FlushDecompress::Sync,
            )?;
            match status {
                Status::Ok => {
                    output_buf.append(&mut out);
                    out = Vec::with_capacity(self.read_buf.len() * 2);
                    let bytes_after = self.inflate.total_in();
                    read_offset = read_offset + (bytes_after - bytes_before) as usize;
                    if read_offset >= self.read_buf.len() {
                        self.read_buf.clear();
                        log::trace!(
                            "Decompression bytes - Input {}b -> Output {}b | Factor: x{:.2}",
                            size_in,
                            output_buf.len(),
                            output_buf.len() as f64 / size_in as f64
                        );
                        return Ok(output_buf);
                    }
                }
                Status::BufError | Status::StreamEnd => {
                    self.read_buf.clear();
                    output_buf.append(&mut out);
                    output_buf.shrink_to_fit();

                    log::trace!(
                        "Decompression bytes - Input {}b -> Output {}b | Factor: x{:.2}",
                        size_in,
                        output_buf.len(),
                        output_buf.len() as f64 / size_in as f64
                    );

                    return Ok(output_buf);
                }
            }
        }
    }
}

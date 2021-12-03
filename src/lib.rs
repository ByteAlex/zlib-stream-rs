#[cfg(feature = "stream")]
pub mod stream;

#[cfg(test)]
mod test;

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
const DEFAULT_OUTPUT_BUFFER_SIZE: usize = 1024 * 128; // 128 kb

pub struct ZlibStreamDecompressor {
    inflate: Decompress,
    read_buf: Vec<u8>,
    output_buffer_factor: Option<usize>,
    output_buffer_size: Option<usize>,
}

impl ZlibStreamDecompressor {
    /// Creates a new ZlibStreamDecompressor with the default configuration
    ///
    /// -> Uses a default output buffer of 128 kb
    pub fn new() -> Self {
        return ZlibStreamDecompressor::with_buffer_size(DEFAULT_OUTPUT_BUFFER_SIZE);
    }

    /// Creates a new ZlibStreamDecompressor with the given output buffer factor
    ///
    /// The factor means that the output buffers size will be dependent on the
    /// read buffers size.
    ///
    /// This is a possible attack vector if your input is not verified, as it can easily
    /// consume a lot of memory if there's no ZLIB_END signature for a long time
    pub fn with_buffer_factor(output_buffer_factor: usize) -> Self {
        Self {
            inflate: Decompress::new(true),
            read_buf: Vec::new(),
            output_buffer_factor: Some(output_buffer_factor),
            output_buffer_size: None,
        }
    }

    /// Creates a new ZlibStreamDecompressor with the given output buffer size
    ///
    /// The buffer size will be fixed at the given value
    pub fn with_buffer_size(output_buffer_size: usize) -> Self {
        Self {
            inflate: Decompress::new(true),
            read_buf: Vec::new(),
            output_buffer_factor: None,
            output_buffer_size: Some(output_buffer_size),
        }
    }

    /// Append the current frame to the read buffer and decompress it if the buffer
    /// ends with a ZLIB_END signature
    ///
    /// This method returns a ZlibDecompressionError::NeedMoreData if the frame does
    /// not end with a ZLIB_END signature
    ///
    /// If the given frame is invalid this method returns a ZlibDecompressionError::DecompressError
    /// this most likely means the state of the entire compressor went out of sync and it should
    /// be recreated.
    ///
    /// In case everything went `Ok`, it will return a Vec<u8> representing the
    /// decompressed data
    pub fn decompress<T: AsRef<[u8]>>(
        &mut self,
        frame: T,
    ) -> Result<Vec<u8>, ZlibDecompressionError> {
        let mut read_buf = frame.as_ref();
        if !read_buf.ends_with(&ZLIB_END_BUF) {
            self.read_buf = read_buf.to_vec();
            return Err(ZlibDecompressionError::NeedMoreData);
        }

        if !self.read_buf.is_empty() {
            self.read_buf.extend_from_slice(read_buf);
            read_buf = self.read_buf.as_slice();
        }

        let size_in = read_buf.len();
        let mut read_offset = 0usize;
        let mut out = self.generate_output_buffer(read_buf.len());
        let mut output_buf = vec![];
        loop {
            let bytes_before = self.inflate.total_in();
            let status = self.inflate.decompress_vec(
                &read_buf[read_offset..],
                &mut out,
                FlushDecompress::Sync,
            )?;
            match status {
                Status::Ok => {
                    output_buf.append(&mut out);
                    out = self.generate_output_buffer(read_buf.len());
                    let bytes_after = self.inflate.total_in();
                    read_offset = read_offset + (bytes_after - bytes_before) as usize;
                    if read_offset >= read_buf.len() {
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

    /// Generates a new output buffer based of the given configuration
    fn generate_output_buffer(&self, frame_size: usize) -> Vec<u8> {
        return if let Some(buffer_factor) = self.output_buffer_factor {
            Vec::with_capacity(frame_size * buffer_factor)
        } else {
            Vec::with_capacity(
                self.output_buffer_size
                    .unwrap_or(DEFAULT_OUTPUT_BUFFER_SIZE),
            )
        };
    }
}

impl Default for ZlibStreamDecompressor {
    fn default() -> Self {
        ZlibStreamDecompressor::new()
    }
}

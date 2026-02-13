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
    #[error("Buffered zlib stream exceeded {limit} bytes (attempted {buffered} bytes)")]
    BufferLimitExceeded { limit: usize, buffered: usize },
}

const ZLIB_END_BUF: [u8; 4] = [0, 0, 255, 255];
const DEFAULT_OUTPUT_BUFFER_SIZE: usize = 1024 * 128; // 128 kb
const DEFAULT_READ_BUFFER_SIZE: usize = 1024 * 8; // 8 kb
const DEFAULT_MAX_BUFFERED_BYTES: usize = 1024 * 1024 * 8; // 8 mb

#[derive(Clone, Copy)]
enum OutputBufferMode {
    Factor(usize),
    Fixed(usize),
}

pub struct ZlibStreamDecompressor {
    inflate: Decompress,
    read_buf: Vec<u8>,
    output_buf: Vec<u8>,
    output_buffer_mode: OutputBufferMode,
    max_buffered_bytes: usize,
}

impl ZlibStreamDecompressor {
    /// Creates a new ZlibStreamDecompressor with the default configuration
    ///
    /// -> Uses a default output buffer of 128 kb
    pub fn new() -> Self {
        ZlibStreamDecompressor::with_buffer_size(DEFAULT_OUTPUT_BUFFER_SIZE)
    }

    /// Creates a new ZlibStreamDecompressor with the given output buffer factor
    ///
    /// The factor means that the output buffers size will be dependent on the
    /// read buffers size.
    ///
    /// This is a possible attack vector if your input is not verified, as it can easily
    /// consume a lot of memory if there's no ZLIB_END signature for a long time
    pub fn with_buffer_factor(output_buffer_factor: usize) -> Self {
        Self::with_buffer_factor_and_limit(output_buffer_factor, DEFAULT_MAX_BUFFERED_BYTES)
    }

    /// Creates a new ZlibStreamDecompressor with an output buffer factor and an explicit
    /// maximum number of bytes allowed while buffering partial frames.
    pub fn with_buffer_factor_and_limit(
        output_buffer_factor: usize,
        max_buffered_bytes: usize,
    ) -> Self {
        Self {
            inflate: Decompress::new(true),
            read_buf: Vec::with_capacity(DEFAULT_READ_BUFFER_SIZE),
            output_buf: Vec::new(),
            output_buffer_mode: OutputBufferMode::Factor(output_buffer_factor),
            max_buffered_bytes: max_buffered_bytes.max(1),
        }
    }

    /// Creates a new ZlibStreamDecompressor with the given output buffer size
    ///
    /// The buffer size will be fixed at the given value
    pub fn with_buffer_size(output_buffer_size: usize) -> Self {
        Self::with_buffer_size_and_limit(output_buffer_size, DEFAULT_MAX_BUFFERED_BYTES)
    }

    /// Creates a new ZlibStreamDecompressor with a fixed output buffer size and an explicit
    /// maximum number of bytes allowed while buffering partial frames.
    pub fn with_buffer_size_and_limit(
        output_buffer_size: usize,
        max_buffered_bytes: usize,
    ) -> Self {
        Self {
            inflate: Decompress::new(true),
            read_buf: Vec::with_capacity(DEFAULT_READ_BUFFER_SIZE),
            output_buf: Vec::new(),
            output_buffer_mode: OutputBufferMode::Fixed(output_buffer_size),
            max_buffered_bytes: max_buffered_bytes.max(1),
        }
    }

    /// Sets the maximum amount of buffered partial zlib frame bytes before returning
    /// `ZlibDecompressionError::BufferLimitExceeded`.
    pub fn set_max_buffered_bytes(&mut self, max_buffered_bytes: usize) {
        self.max_buffered_bytes = max_buffered_bytes.max(1);
        if self.read_buf.len() > self.max_buffered_bytes {
            self.read_buf.clear();
        }
    }

    /// Builder variant of `set_max_buffered_bytes`.
    pub fn with_max_buffered_bytes(mut self, max_buffered_bytes: usize) -> Self {
        self.set_max_buffered_bytes(max_buffered_bytes);
        self
    }

    /// Returns the current maximum partial-frame buffering limit in bytes.
    pub fn max_buffered_bytes(&self) -> usize {
        self.max_buffered_bytes
    }

    /// Clears decompression state and buffered input/output data.
    pub fn reset(&mut self) {
        self.inflate = Decompress::new(true);
        self.read_buf.clear();
        self.output_buf.clear();
    }

    /// Reduces retained capacity of internal buffers after traffic spikes.
    pub fn trim_buffers(&mut self) {
        self.read_buf.shrink_to(DEFAULT_READ_BUFFER_SIZE);
        self.output_buf
            .shrink_to(Self::output_capacity_hint(self.output_buffer_mode));
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
        let mut output = Vec::new();
        self.decompress_into(frame, &mut output)?;
        Ok(output)
    }

    /// Decompresses the provided frame into an internal reusable output buffer.
    ///
    /// The returned slice stays valid until the next mutable call on this decompressor.
    pub fn decompress_ref<T: AsRef<[u8]>>(
        &mut self,
        frame: T,
    ) -> Result<&[u8], ZlibDecompressionError> {
        let frame = frame.as_ref();
        let output_buffer_mode = self.output_buffer_mode;
        let max_buffered_bytes = self.max_buffered_bytes;
        let inflate = &mut self.inflate;
        let read_buf = &mut self.read_buf;
        let output_buf = &mut self.output_buf;

        Self::decompress_impl(
            inflate,
            read_buf,
            output_buffer_mode,
            max_buffered_bytes,
            frame,
            output_buf,
        )?;
        Ok(output_buf.as_slice())
    }

    /// Appends decompressed data to a caller-provided output buffer.
    ///
    /// Reusing one output buffer across calls avoids repeated allocations and is
    /// the highest throughput path for hot loops.
    pub fn decompress_into<T: AsRef<[u8]>>(
        &mut self,
        frame: T,
        output_buf: &mut Vec<u8>,
    ) -> Result<(), ZlibDecompressionError> {
        let frame = frame.as_ref();
        let output_buffer_mode = self.output_buffer_mode;
        let max_buffered_bytes = self.max_buffered_bytes;
        Self::decompress_impl(
            &mut self.inflate,
            &mut self.read_buf,
            output_buffer_mode,
            max_buffered_bytes,
            frame,
            output_buf,
        )
    }

    #[cfg(feature = "bytes-api")]
    /// Convenience API for users that consume `bytes::Bytes`.
    pub fn decompress_bytes<T: AsRef<[u8]>>(
        &mut self,
        frame: T,
    ) -> Result<bytes::Bytes, ZlibDecompressionError> {
        self.decompress(frame).map(bytes::Bytes::from)
    }

    #[cfg(feature = "bytes-api")]
    /// Convenience API for users that maintain a `bytes::BytesMut` output buffer.
    pub fn decompress_into_bytes_mut<T: AsRef<[u8]>>(
        &mut self,
        frame: T,
        output_buf: &mut bytes::BytesMut,
    ) -> Result<(), ZlibDecompressionError> {
        let decompressed = self.decompress_ref(frame)?;
        output_buf.clear();
        let reserve_bytes = decompressed.len().saturating_sub(output_buf.capacity());
        if reserve_bytes > 0 {
            output_buf.reserve(reserve_bytes);
        }
        output_buf.extend_from_slice(decompressed);
        Ok(())
    }

    #[inline]
    fn output_capacity_for_mode(output_buffer_mode: OutputBufferMode, frame_size: usize) -> usize {
        match output_buffer_mode {
            OutputBufferMode::Factor(buffer_factor) => frame_size.saturating_mul(buffer_factor),
            OutputBufferMode::Fixed(buffer_size) => buffer_size,
        }
    }

    #[inline]
    fn output_capacity_hint(output_buffer_mode: OutputBufferMode) -> usize {
        match output_buffer_mode {
            OutputBufferMode::Factor(_) => DEFAULT_OUTPUT_BUFFER_SIZE,
            OutputBufferMode::Fixed(buffer_size) => buffer_size,
        }
    }

    #[inline]
    fn buffer_partial_frame(
        read_buf: &mut Vec<u8>,
        frame: &[u8],
        max_buffered_bytes: usize,
    ) -> Result<(), ZlibDecompressionError> {
        let buffered = read_buf.len().saturating_add(frame.len());
        if buffered > max_buffered_bytes {
            read_buf.clear();
            return Err(ZlibDecompressionError::BufferLimitExceeded {
                limit: max_buffered_bytes,
                buffered,
            });
        }

        read_buf.reserve(frame.len());
        read_buf.extend_from_slice(frame);
        Ok(())
    }

    fn decompress_impl(
        inflate: &mut Decompress,
        read_buf: &mut Vec<u8>,
        output_buffer_mode: OutputBufferMode,
        max_buffered_bytes: usize,
        frame: &[u8],
        output_buf: &mut Vec<u8>,
    ) -> Result<(), ZlibDecompressionError> {
        if read_buf.is_empty() {
            if !frame.ends_with(&ZLIB_END_BUF) {
                Self::buffer_partial_frame(read_buf, frame, max_buffered_bytes)?;
                return Err(ZlibDecompressionError::NeedMoreData);
            }

            let output_capacity = Self::output_capacity_for_mode(output_buffer_mode, frame.len());
            return Ok(Self::decompress_inner(
                inflate,
                frame,
                output_capacity,
                output_buf,
            )?);
        }

        Self::buffer_partial_frame(read_buf, frame, max_buffered_bytes)?;
        if !read_buf.ends_with(&ZLIB_END_BUF) {
            return Err(ZlibDecompressionError::NeedMoreData);
        }

        let output_capacity = Self::output_capacity_for_mode(output_buffer_mode, read_buf.len());
        let output =
            Self::decompress_inner(inflate, read_buf.as_slice(), output_capacity, output_buf);
        read_buf.clear();
        Ok(output?)
    }

    fn decompress_inner(
        inflate: &mut Decompress,
        read_buf: &[u8],
        output_capacity: usize,
        output_buf: &mut Vec<u8>,
    ) -> Result<(), DecompressError> {
        let size_in = read_buf.len();
        let mut read_offset = 0usize;
        output_buf.clear();

        let reserve_bytes = output_capacity.saturating_sub(output_buf.capacity());
        if reserve_bytes > 0 {
            output_buf.reserve(reserve_bytes);
        }

        // flate2 can report Status::BufError when the output buffer needs to grow;
        // treat it as a signal to reserve more and continue instead of returning a truncated frame.
        let mut no_progress_spins: usize = 0;
        const MAX_NO_PROGRESS_SPINS: usize = 128;

        loop {
            let bytes_before = inflate.total_in();
            let out_before = output_buf.len();
            let status = inflate.decompress_vec(
                &read_buf[read_offset..],
                output_buf,
                FlushDecompress::Sync,
            )?;
            let bytes_after = inflate.total_in();
            let bytes_read = (bytes_after - bytes_before) as usize;
            read_offset = read_offset.saturating_add(bytes_read);

            let made_progress = bytes_read > 0 || output_buf.len() > out_before;
            if made_progress {
                no_progress_spins = 0;
            } else {
                no_progress_spins += 1;
                if no_progress_spins > MAX_NO_PROGRESS_SPINS {
                    // Avoid a tight infinite loop on malformed streams.
                    break;
                }
            }

            match status {
                Status::Ok => {
                    if read_offset < size_in {
                        continue;
                    }
                }
                Status::BufError => {
                    if read_offset < size_in {
                        // Input remains; grow output capacity and keep draining.
                        output_buf.reserve(output_buf.capacity().max(DEFAULT_OUTPUT_BUFFER_SIZE));
                        continue;
                    }
                }
                Status::StreamEnd => {
                    // End-of-stream for this flate stream.
                }
            }

            let factor = if size_in == 0 { 0.0 } else { output_buf.len() as f64 / size_in as f64 };
            log::trace!(
                "Decompression bytes - Input {}b -> Output {}b | Factor: x{:.2}",
                size_in,
                output_buf.len(),
                factor
            );
            return Ok(());
        }

        let factor = if size_in == 0 { 0.0 } else { output_buf.len() as f64 / size_in as f64 };
        log::trace!(
            "Decompression bytes - Input {}b -> Output {}b | Factor: x{:.2}",
            size_in,
            output_buf.len(),
            factor
        );
        Ok(())
    }

}

impl Default for ZlibStreamDecompressor {
    fn default() -> Self {
        ZlibStreamDecompressor::new()
    }
}

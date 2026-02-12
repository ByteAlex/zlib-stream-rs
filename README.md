# zlib-stream-rs

A simple utility crate to make decompressing from zlib-stream easier.

This crate is based of [flate2](https://github.com/rust-lang/flate2-rs) and their 
[clouflare zlib backend](https://github.com/cloudflare/zlib).

## Usage
1. StreamExt using `stream` feature
```rust
use zlib_stream::stream::ZlibStream; 

async fn setup<V: AsRef<[u8]> + Sized, T: Stream<Item=V> + Unpin>(stream: T) {
    let mut stream = ZlibStream::new(stream);
    
    loop {
        let data: Option<Result<Vec<u8>, zlib_stream::ZlibDecompressionError>> = stream.next().await;
        do_something(data);
    }
}
```
2. Barebone Implementation
```rust
use zlib_stream::{ZlibStreamDecompressor, ZlibDecompressionError};

fn worker_loop() {
    let mut decompress: ZlibStreamDecompressor = ZlibStreamDecompressor::new();
    
    loop {
        let mut frame: Vec<u8> = get_compressed_frame();
        match decompress.decompress(frame) {
            Ok(vec) => process_data(vec),
            Err(ZlibDecompressionError::NeedMoreData) => continue,
            Err(_err) => panic!("Broken frame!"),
        }
    }
}
```

3. High-throughput (reused output buffer)
```rust
use zlib_stream::{ZlibDecompressionError, ZlibStreamDecompressor};

fn worker_loop() {
    let mut decompress = ZlibStreamDecompressor::new();
    let mut output = Vec::with_capacity(1024 * 128);

    loop {
        let frame: Vec<u8> = get_compressed_frame();
        match decompress.decompress_into(frame, &mut output) {
            Ok(()) => process_data(&output),
            Err(ZlibDecompressionError::NeedMoreData) => continue,
            Err(_err) => panic!("Broken frame!"),
        }
    }
}
```

4. Highest throughput (internal reusable output buffer)
```rust
use zlib_stream::{ZlibDecompressionError, ZlibStreamDecompressor};

fn worker_loop() {
    let mut decompress = ZlibStreamDecompressor::new();

    loop {
        let frame: Vec<u8> = get_compressed_frame();
        match decompress.decompress_ref(frame) {
            Ok(data) => process_data(data),
            Err(ZlibDecompressionError::NeedMoreData) => continue,
            Err(_err) => panic!("Broken frame!"),
        }
    }
}
```

5. Memory protection for fragmented frames
```rust
use zlib_stream::ZlibStreamDecompressor;

let mut decompress = ZlibStreamDecompressor::with_buffer_size_and_limit(
    1024 * 128, // output buffer size
    1024 * 1024 // max buffered partial-frame bytes
);

// after protocol errors/out-of-sync streams
decompress.reset();
```

6. `bytes` integration (`bytes-api` feature)
```rust
use bytes::Bytes;
use zlib_stream::ZlibStreamDecompressor;

let mut decompress = ZlibStreamDecompressor::new();
let frame: Bytes = get_bytes_frame();
let payload = decompress.decompress_bytes(frame)?;
```

## Benchmarking
Run the Criterion suite:

```bash
cargo bench --bench decompress
```

## Release Tuning
For maximum throughput in production binaries:

1. Use CPU-specific code generation when possible:
```bash
RUSTFLAGS="-C target-cpu=native" cargo build --release
```

2. Consider these release profile settings in the consuming binary:
```toml
[profile.release]
lto = "fat"
codegen-units = 1
```

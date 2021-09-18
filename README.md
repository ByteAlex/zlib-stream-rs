# zlib-stream-rs

A simple utility crate to make decompressing from zlib-stream easier.

This crate is based of [flate2](https://github.com/rust-lang/flate2-rs) and their 
[clouflare zlib backend](https://github.com/cloudflare/zlib).

## Usage
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
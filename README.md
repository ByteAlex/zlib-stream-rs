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
        let data: Option<Result<Vec<u8>, flate2::DecompressError>> = stream.next().await;
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
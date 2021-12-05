#[cfg(feature = "chunk")]
use crate::chunk::ChunkedByteStream;
#[cfg(feature = "stream")]
use crate::stream::ZlibStream;
use crate::{ZlibDecompressionError, ZlibStreamDecompressor};
use futures_util::StreamExt;

fn payload() -> Vec<u8> {
    vec![
        120, 156, 52, 201, 65, 14, 130, 48, 16, 5, 208, 187, 252, 117, 107, 90, 35, 155, 185, 10,
        37, 100, 132, 137, 54, 41, 5, 203, 160, 49, 77, 239, 46, 27, 119, 47, 121, 21, 10, 202, 71,
        74, 6, 251, 31, 235, 6, 242, 206, 96, 6, 85, 60, 133, 139, 222, 133, 117, 140, 89, 165,
        188, 57, 129, 110, 254, 218, 157, 63, 106, 225, 73, 64, 61, 250, 128, 7, 171, 124, 248,
        107, 183, 50, 219, 133, 99, 182, 154, 253, 43, 192, 212, 128, 37, 78, 101, 221, 3, 200, 93,
        92, 27, 48, 180, 246, 3, 0, 0, 255, 255,
    ]
}

fn split_payload() -> Vec<Vec<u8>> {
    vec![
        vec![
            120, 156, 52, 201, 65, 14, 130, 48, 16, 5, 208, 187, 252, 117, 107, 90, 35, 155, 185,
            10, 37, 100, 132, 137, 54, 41, 5, 203, 160, 49, 77, 239, 46, 27, 119, 47, 121, 21, 10,
            202, 71,
        ],
        vec![
            74, 6, 251, 31, 235, 6, 242, 206, 96, 6, 85, 60, 133, 139, 222, 133, 117, 140, 89, 165,
            188, 57, 129, 110, 254, 218, 157, 63, 106, 225, 73, 64, 61, 250, 128, 7, 171, 124, 248,
            107, 183, 50, 219, 133, 99, 182, 154, 253, 43, 192, 212, 128, 37, 78, 101, 221, 3, 200,
            93, 92, 27, 48, 180, 246, 3, 0, 0, 255, 255,
        ],
    ]
}

fn inflated() -> &'static str {
    r#"{"t":null,"s":null,"op":10,"d":{"heartbeat_interval":41250,"_trace":["[\"gateway-prd-main-tn1q\",{\"micros\":0.0}]"]}}"#
}

#[test]
fn test() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    let result = decompressor.decompress(payload());
    assert_eq!(
        inflated(),
        String::from_utf8(result.expect("Decompression failed")).unwrap()
    )
}

#[test]
fn test_split() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    let vec = split_payload();
    let mut payloads = vec.iter();
    let result = decompressor.decompress(payloads.next().expect("Missing payload"));
    assert!(
        match result {
            Err(ZlibDecompressionError::NeedMoreData) => true,
            _ => false,
        },
        "First non-zlib payload didn't return NeedMoreData"
    );
    let result = decompressor.decompress(payloads.next().expect("Missing payload"));
    assert_eq!(
        inflated(),
        String::from_utf8(result.expect("Decompression failed")).unwrap()
    )
}

#[cfg(feature = "stream")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stream() {
    let stream: Vec<Vec<u8>> = vec![payload()];

    let stream = futures_util::stream::iter(stream);
    let mut stream = ZlibStream::new(stream);

    let result = stream.next().await;
    assert_eq!(
        inflated(),
        String::from_utf8(
            result
                .expect("Poll returned end of stream")
                .expect("Decompression failed")
        )
        .unwrap()
    )
}

#[cfg(feature = "stream")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stream_split() {
    let stream: Vec<Vec<u8>> = split_payload();

    let stream = futures_util::stream::iter(stream);
    let mut stream = ZlibStream::new(stream);

    let result = stream.next().await;

    assert_eq!(
        inflated(),
        String::from_utf8(
            result
                .expect("Poll returned end of stream")
                .expect("Decompression failed")
        )
        .unwrap()
    )
}

#[cfg(feature = "chunk")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_chunk_stream() {
    let chunk_size = 8usize;
    let data = vec![payload()];

    let stream = futures_util::stream::iter(data);
    let mut stream = ChunkedByteStream::new(stream, chunk_size);
    let mut concat = vec![];
    while let Some(data) = stream.next().await {
        concat.extend_from_slice(data.as_slice());
        assert!(data.len() <= chunk_size, "Data size exceeded threshold!")
    }

    assert_eq!(concat, payload(), "Payloads aren't equal")
}

#[cfg(feature = "chunk")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_chunk_stream_zlib() {
    let chunk_size = 55usize;
    let data = vec![payload()];

    let stream = futures_util::stream::iter(data);
    let stream = ChunkedByteStream::new(stream, chunk_size);
    let mut stream = ZlibStream::new(stream);

    let result = stream.next().await;
    assert_eq!(
        inflated(),
        String::from_utf8(
            result
                .expect("Poll returned end of stream")
                .expect("Decompression failed")
        )
        .unwrap()
    )
}

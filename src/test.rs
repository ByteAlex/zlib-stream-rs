use crate::stream::ZlibStream;
use crate::{ZlibDecompressionError, ZlibStreamDecompressor};
use futures_util::{Stream, StreamExt};
use std::pin::Pin;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::Write;

fn compress_to_zlib_stream_frame(input: &[u8]) -> Vec<u8> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::default());
    encoder.write_all(input).expect("write to encoder");
    let mut out = encoder.finish().expect("finish encoder");
    // Discord zlib-stream frames are terminated by 0x00 0x00 0xff 0xff.
    if !out.ends_with(&[0, 0, 255, 255]) {
        out.extend_from_slice(&[0, 0, 255, 255]);
    }
    out
}

#[cfg(feature = "bytes-api")]
use crate::stream::ZlibBytesStream;
#[cfg(feature = "bytes-api")]
use bytes::{Bytes, BytesMut};

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

fn split_payload_three() -> Vec<Vec<u8>> {
    vec![
        vec![
            120, 156, 52, 201, 65, 14, 130, 48, 16, 5, 208, 187, 252, 117, 107, 90, 35, 155, 185,
            10,
        ],
        vec![
            37, 100, 132, 137, 54, 41, 5, 203, 160, 49, 77, 239, 46, 27, 119, 47, 121, 21, 10, 202,
            71, 74, 6, 251, 31, 235, 6, 242, 206, 96, 6, 85, 60, 133, 139, 222, 133, 117, 140, 89,
        ],
        vec![
            165, 188, 57, 129, 110, 254, 218, 157, 63, 106, 225, 73, 64, 61, 250, 128, 7, 171, 124,
            248, 107, 183, 50, 219, 133, 99, 182, 154, 253, 43, 192, 212, 128, 37, 78, 101, 221, 3,
            200, 93, 92, 27, 48, 180, 246, 3, 0, 0, 255, 255,
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
fn test_decompress_into_reused_buffer() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    let mut output = Vec::with_capacity(1024);

    decompressor
        .decompress_into(payload(), &mut output)
        .expect("Decompression failed");
    assert_eq!(inflated(), std::str::from_utf8(&output).unwrap());

    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    output.clear();
    let frames = split_payload();
    assert!(matches!(
        decompressor.decompress_into(frames[0].as_slice(), &mut output),
        Err(ZlibDecompressionError::NeedMoreData)
    ));
    decompressor
        .decompress_into(frames[1].as_slice(), &mut output)
        .expect("Decompression failed");
    assert_eq!(inflated(), std::str::from_utf8(&output).unwrap());
}

#[test]
fn test_decompress_ref_reused_output_buffer() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    let result = decompressor
        .decompress_ref(payload())
        .expect("Decompression failed");
    assert_eq!(inflated(), std::str::from_utf8(result).unwrap());

    decompressor.reset();
    let frames = split_payload();
    assert!(matches!(
        decompressor.decompress_ref(frames[0].as_slice()),
        Err(ZlibDecompressionError::NeedMoreData)
    ));
    let result = decompressor
        .decompress_ref(frames[1].as_slice())
        .expect("Decompression failed");
    assert_eq!(inflated(), std::str::from_utf8(result).unwrap());
}

#[test]
fn test_split() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    let vec = split_payload();
    let mut payloads = vec.iter();
    let result = decompressor.decompress(payloads.next().expect("Missing payload"));
    assert!(
        matches!(result, Err(ZlibDecompressionError::NeedMoreData)),
        "First non-zlib payload didn't return NeedMoreData"
    );
    let result = decompressor.decompress(payloads.next().expect("Missing payload"));
    assert_eq!(
        inflated(),
        String::from_utf8(result.expect("Decompression failed")).unwrap()
    )
}

#[test]
fn test_split_three_frames() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    let frames = split_payload_three();
    let mut iter = frames.iter();

    let result = decompressor.decompress(iter.next().expect("Missing payload"));
    assert!(
        matches!(result, Err(ZlibDecompressionError::NeedMoreData)),
        "First non-zlib payload didn't return NeedMoreData"
    );

    let result = decompressor.decompress(iter.next().expect("Missing payload"));
    assert!(
        matches!(result, Err(ZlibDecompressionError::NeedMoreData)),
        "Second non-zlib payload didn't return NeedMoreData"
    );

    let result = decompressor.decompress(iter.next().expect("Missing payload"));
    assert_eq!(
        inflated(),
        String::from_utf8(result.expect("Decompression failed")).unwrap()
    )
}

#[test]
fn test_buffer_limit_exceeded() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor_and_limit(8, 8);
    let result = decompressor.decompress(vec![1u8; 9]);

    assert!(matches!(
        result,
        Err(ZlibDecompressionError::BufferLimitExceeded {
            limit: 8,
            buffered: 9
        })
    ));
}

#[test]
fn test_reset_after_bad_data() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_size(1024);
    let _ = decompressor.decompress(vec![1u8; 32]);
    decompressor.reset();

    let result = decompressor.decompress(payload());
    assert_eq!(
        inflated(),
        String::from_utf8(result.expect("Decompression failed")).unwrap()
    );
}

#[test]
fn test_buferror_grows_output_buffer() {
    // Highly compressible payload: small input frame, multi-megabyte output.
    let large = "a".repeat(2 * 1024 * 1024);
    let expected = format!("{{\"t\":\"GUILD_CREATE\",\"d\":{{\"blob\":\"{}\"}}}}", large);

    let frame = compress_to_zlib_stream_frame(expected.as_bytes());

    // Intentionally small initial capacity heuristic; implementation must grow as needed.
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(1);
    let out = decompressor.decompress(frame).expect("decompression failed");

    assert_eq!(expected.as_bytes(), out.as_slice());
}

#[cfg(feature = "stream")]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stream() {
    let stream: Vec<Vec<u8>> = vec![payload()];

    let stream = futures_util::stream::iter(stream);
    let mut stream = ZlibStream::new(stream);

    let result = futures_util::future::poll_fn(move |cx| Pin::new(&mut stream).poll_next(cx)).await;
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

#[cfg(all(feature = "stream", feature = "bytes-api"))]
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn test_stream_bytes() {
    let stream: Vec<Bytes> = vec![Bytes::from(payload())];

    let stream = futures_util::stream::iter(stream);
    let mut stream = ZlibBytesStream::new(stream);

    let result = stream.next().await;

    assert_eq!(
        inflated(),
        std::str::from_utf8(
            result
                .expect("Poll returned end of stream")
                .expect("Decompression failed")
                .as_ref(),
        )
        .unwrap()
    )
}

#[cfg(feature = "bytes-api")]
#[test]
fn test_bytes_apis() {
    let mut decompressor = ZlibStreamDecompressor::with_buffer_factor(8);
    let frame = Bytes::from(payload());
    let out = decompressor
        .decompress_bytes(frame.clone())
        .expect("Decompression failed");
    assert_eq!(inflated(), std::str::from_utf8(out.as_ref()).unwrap());

    let mut output = BytesMut::new();
    decompressor.reset();
    decompressor
        .decompress_into_bytes_mut(frame, &mut output)
        .expect("Decompression failed");
    assert_eq!(inflated(), std::str::from_utf8(output.as_ref()).unwrap());
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

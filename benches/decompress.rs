use criterion::{
    black_box, criterion_group, criterion_main, BatchSize, BenchmarkId, Criterion, Throughput,
};
use flate2::{Compress, CompressError, Compression, FlushCompress, Status};
use zlib_stream::{ZlibDecompressionError, ZlibStreamDecompressor};

const ZLIB_END_BUF: [u8; 4] = [0, 0, 255, 255];
const STREAM_FRAMES_PER_ITER: usize = 128;
const PAYLOAD_SIZES: [usize; 4] = [256, 1024, 8192, 65536];

struct BenchCase {
    size: usize,
    frames: Vec<Vec<u8>>,
    split2: (Vec<u8>, Vec<u8>),
    split3: (Vec<u8>, Vec<u8>, Vec<u8>),
}

fn build_payload(target_len: usize) -> Vec<u8> {
    let mut payload = Vec::with_capacity(target_len);
    let mut state = 0x1234_5678u32;

    while payload.len() < target_len {
        state = state.wrapping_mul(1_664_525).wrapping_add(1_013_904_223);
        let chunk = format!(
            "{{\"s\":{},\"x\":\"{:08x}\",\"ok\":true,\"kind\":\"bench\"}}",
            payload.len(),
            state
        );
        payload.extend_from_slice(chunk.as_bytes());
    }

    payload.truncate(target_len);
    payload
}

fn compress_sync_frame(compress: &mut Compress, payload: &[u8]) -> Result<Vec<u8>, CompressError> {
    let mut out = Vec::with_capacity((payload.len() / 2).max(64));
    let status = compress.compress_vec(payload, &mut out, FlushCompress::Sync)?;
    debug_assert!(matches!(
        status,
        Status::Ok | Status::BufError | Status::StreamEnd
    ));
    assert!(
        out.ends_with(&ZLIB_END_BUF),
        "compressed frame must end with zlib sync flush marker"
    );
    Ok(out)
}

fn split_in_two(frame: &[u8]) -> (Vec<u8>, Vec<u8>) {
    assert!(frame.len() > ZLIB_END_BUF.len());
    let cut = frame.len() - ZLIB_END_BUF.len();
    (frame[..cut].to_vec(), frame[cut..].to_vec())
}

fn split_in_three(frame: &[u8]) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    assert!(frame.len() > ZLIB_END_BUF.len() + 2);
    let cut2 = frame.len() - ZLIB_END_BUF.len();
    let cut1 = cut2 / 2;
    (
        frame[..cut1].to_vec(),
        frame[cut1..cut2].to_vec(),
        frame[cut2..].to_vec(),
    )
}

fn build_case(size: usize) -> BenchCase {
    let payload = build_payload(size);
    let mut compress = Compress::new(Compression::default(), true);
    let mut frames = Vec::with_capacity(STREAM_FRAMES_PER_ITER);
    for _ in 0..STREAM_FRAMES_PER_ITER {
        frames.push(compress_sync_frame(&mut compress, &payload).expect("compression failed"));
    }

    let split2 = split_in_two(frames[0].as_slice());
    let split3 = split_in_three(frames[0].as_slice());

    // Setup-time correctness guard to ensure benchmarks run on valid stream data.
    let mut decompressor = ZlibStreamDecompressor::new();
    for frame in &frames {
        let out = decompressor
            .decompress(frame.as_slice())
            .expect("frame should be valid");
        assert_eq!(out.len(), size);
    }

    BenchCase {
        size,
        frames,
        split2,
        split3,
    }
}

fn bench_stream(c: &mut Criterion) {
    let cases: Vec<BenchCase> = PAYLOAD_SIZES.iter().map(|size| build_case(*size)).collect();
    let mut group = c.benchmark_group("decompress_stream");

    for case in &cases {
        let bytes = (case.size * case.frames.len()) as u64;
        group.throughput(Throughput::Bytes(bytes));

        group.bench_with_input(
            BenchmarkId::new("decompress_alloc", case.size),
            case,
            |b, case| {
                b.iter(|| {
                    let mut decompressor = ZlibStreamDecompressor::new();
                    for frame in &case.frames {
                        let out = decompressor
                            .decompress(black_box(frame.as_slice()))
                            .expect("decompression failed");
                        black_box(out);
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("decompress_into_reuse", case.size),
            case,
            |b, case| {
                b.iter(|| {
                    let mut decompressor = ZlibStreamDecompressor::new();
                    let mut out = Vec::with_capacity(case.size * 2);
                    for frame in &case.frames {
                        decompressor
                            .decompress_into(black_box(frame.as_slice()), &mut out)
                            .expect("decompression failed");
                        black_box(out.as_slice());
                    }
                });
            },
        );

        group.bench_with_input(
            BenchmarkId::new("decompress_ref_borrowed", case.size),
            case,
            |b, case| {
                b.iter(|| {
                    let mut decompressor = ZlibStreamDecompressor::new();
                    for frame in &case.frames {
                        let out = decompressor
                            .decompress_ref(black_box(frame.as_slice()))
                            .expect("decompression failed");
                        black_box(out);
                    }
                });
            },
        );
    }

    group.finish();
}

fn bench_split(c: &mut Criterion) {
    let cases: Vec<BenchCase> = PAYLOAD_SIZES.iter().map(|size| build_case(*size)).collect();
    let mut group = c.benchmark_group("decompress_split");

    for case in &cases {
        group.throughput(Throughput::Bytes(case.size as u64));

        group.bench_with_input(
            BenchmarkId::new("split2_decompress", case.size),
            case,
            |b, case| {
                let (first, second) = (&case.split2.0, &case.split2.1);
                b.iter_batched(
                    ZlibStreamDecompressor::new,
                    |mut decompressor| {
                        let first_result = decompressor.decompress(black_box(first.as_slice()));
                        debug_assert!(matches!(
                            first_result,
                            Err(ZlibDecompressionError::NeedMoreData)
                        ));
                        let out = decompressor
                            .decompress(black_box(second.as_slice()))
                            .expect("decompression failed");
                        black_box(out);
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("split2_decompress_into", case.size),
            case,
            |b, case| {
                let (first, second) = (&case.split2.0, &case.split2.1);
                b.iter_batched(
                    || {
                        (
                            ZlibStreamDecompressor::new(),
                            Vec::with_capacity(case.size * 2),
                        )
                    },
                    |(mut decompressor, mut out)| {
                        let first_result =
                            decompressor.decompress_into(black_box(first.as_slice()), &mut out);
                        debug_assert!(matches!(
                            first_result,
                            Err(ZlibDecompressionError::NeedMoreData)
                        ));
                        decompressor
                            .decompress_into(black_box(second.as_slice()), &mut out)
                            .expect("decompression failed");
                        black_box(out.as_slice());
                    },
                    BatchSize::SmallInput,
                );
            },
        );

        group.bench_with_input(
            BenchmarkId::new("split3_decompress", case.size),
            case,
            |b, case| {
                let (first, second, third) = (&case.split3.0, &case.split3.1, &case.split3.2);
                b.iter_batched(
                    ZlibStreamDecompressor::new,
                    |mut decompressor| {
                        let first_result = decompressor.decompress(black_box(first.as_slice()));
                        debug_assert!(matches!(
                            first_result,
                            Err(ZlibDecompressionError::NeedMoreData)
                        ));
                        let second_result = decompressor.decompress(black_box(second.as_slice()));
                        debug_assert!(matches!(
                            second_result,
                            Err(ZlibDecompressionError::NeedMoreData)
                        ));
                        let out = decompressor
                            .decompress(black_box(third.as_slice()))
                            .expect("decompression failed");
                        black_box(out);
                    },
                    BatchSize::SmallInput,
                );
            },
        );
    }

    group.finish();
}

criterion_group!(benches, bench_stream, bench_split);
criterion_main!(benches);

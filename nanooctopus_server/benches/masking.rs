use byte_unit::{Byte, UnitType};
use criterion::{BenchmarkId, Criterion, criterion_group, criterion_main};
use nanooctopus_server::web_socket::PayloadMasker;

fn create_test_data<const N: usize>() -> [u8; N] {
    core::array::from_fn(|i| {
        // Pseudo random
        (i.wrapping_mul(37).wrapping_add(13) % 256) as u8
    })
}
fn label(bytes: usize) -> String {
    let v = Byte::from_u64(bytes as u64).get_appropriate_unit(UnitType::Binary);
    format!("mask_small_chank_{:.0}_{}", v.get_value(), v.get_unit())
}

fn bench_mask_small_chank<const N: usize>(c: &mut Criterion) {
    const MASK_KEY: u32 = 0x01020304;
    let data = create_test_data::<N>();

    let id_str = format!("mask_small_chank_{}", label(N));
    c.bench_with_input(BenchmarkId::new(&id_str, "input"), &data, |b, data| {
        b.iter(|| {
            let mut buf = data.clone();
            for offset in 0..((usize::BITS as usize) / 8) {
                let data_with_offest = &mut buf[offset..];
                PayloadMasker::new(MASK_KEY).mask_small_chank(data_with_offest);
            }
        })
    });
}

fn bench_mask_chank<const N: usize>(c: &mut Criterion) {
    const MASK_KEY: u32 = 0x01020304;
    let data = create_test_data::<N>();

    let id_str = format!("mask_chank_{}", label(N));
    c.bench_with_input(BenchmarkId::new(&id_str, "input"), &data, |b, data| {
        b.iter(|| {
            let mut buf = data.clone();
            for offset in 0..((usize::BITS as usize) / 8) {
                let data_with_offest = &mut buf[offset..];
                PayloadMasker::new(MASK_KEY).mask_chank(data_with_offest);
            }
        })
    });
}

fn bench_mask_chank_simd<const N: usize>(c: &mut Criterion) {
    const MASK_KEY: u32 = 0x01020304;
    let data = create_test_data::<N>();

    let id_str = format!("mask_chank_simd_{}", label(N));
    c.bench_with_input(BenchmarkId::new(&id_str, "input"), &data, |b, data| {
        b.iter(|| {
            let mut buf = data.clone();
            for offset in 0..((usize::BITS as usize) / 8) {
                let data_with_offest = &mut buf[offset..];
                PayloadMasker::new(MASK_KEY).mask_chank_simd(data_with_offest);
            }
        })
    });
}

criterion_group!(
    benches_small_mask,
    bench_mask_small_chank::<4096>,
    bench_mask_small_chank::<1000>,
    bench_mask_small_chank::<512>,
    bench_mask_small_chank::<128>,
    bench_mask_small_chank::<32>,
    bench_mask_small_chank::<7>
);

criterion_group!(
    benches_mask,
    bench_mask_chank::<4096>,
    bench_mask_chank::<1000>,
    bench_mask_chank::<512>,
    bench_mask_chank::<128>,
    bench_mask_chank::<32>,
    bench_mask_chank::<7>
);

criterion_group!(
    benches_mask_simd,
    bench_mask_chank_simd::<4096>,
    bench_mask_chank_simd::<1000>,
    bench_mask_chank_simd::<512>,
    bench_mask_chank_simd::<128>,
    bench_mask_chank_simd::<32>,
    bench_mask_chank_simd::<7>
);

criterion_main!(benches_small_mask, benches_mask, benches_mask_simd);

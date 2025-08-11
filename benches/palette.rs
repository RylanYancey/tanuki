
#![feature(allocator_api)]

use criterion::*;
criterion::criterion_group!(benches, benchmarks);
criterion::criterion_main!(benches);
use tanuki::region::PaletteArray;
use std::hint::black_box;

struct Rng(u64);

impl Rng {
    pub fn next(&mut self) -> u64 {
        const P0: u64 = 0xa076_1d64_78bd_642f;
        const P1: u64 = 0xe703_7ed1_a0b4_28db;
        self.0 = self.0.wrapping_add(P0);
        let r = u128::from(self.0) * u128::from(self.0 ^ P1);
        ((r >> 64) ^ r) as u64
    }
}


fn benchmarks(c: &mut Criterion) {
    let mut palette = PaletteArray::with_palette_capacity(256, std::alloc::Global);
    let mut rng = Rng(0x3787378357835738);
    let vals = (0..128).map(|_| (rng.next() & 127) as u16).collect::<Vec<u16>>();
    for i in 0..32768 {
        unsafe { palette.set(i, vals[i & 127]) };
    }

    c.bench_function("palette-set-8", |b| bench_set::<8>(b))
    .bench_function("palette-set-16", |b| bench_set::<16>(b))
    .bench_function("palette-set-64", |b| bench_set::<64>(b))
    .bench_function("palette-set-128", |b| bench_set::<128>(b))
    .bench_function("palette-set-256", |b| bench_set::<256>(b))
    .bench_function("palette-set-512", |b| bench_set::<512>(b))
    .bench_function("palette-get-8", |b| bench_get::<8>(b))
    .bench_function("palette-get-16", |b| bench_get::<16>(b))
    .bench_function("palette-get-64", |b| bench_get::<64>(b))
    .bench_function("palette-get-128", |b| bench_get::<128>(b))
    .bench_function("palette-get-256", |b| bench_get::<256>(b))
    .bench_function("palette-get-512", |b| bench_get::<512>(b));
}

/// S must be a power of 2
fn bench_set<const S: usize>(b: &mut Bencher) {
    let mut palette = PaletteArray::with_palette_capacity(S, std::alloc::Global);
    let mut rng = Rng(0x375839675189);
    let vals = (0..S).map(|_| (rng.next() % S as u64) as u16).collect::<Vec<_>>();
    for i in 0..32768 {
        unsafe { palette.set(i, vals[i % S]); }
    }

    b.iter(|| {
        for i in black_box(0..32768) {
            let j = i ^ 0x5555;
            black_box(unsafe { palette.set(j, vals[j & (S - 1)])});
        }
    });
}

fn bench_get<const S: usize>(b: &mut Bencher) {
    let mut palette = PaletteArray::with_palette_capacity(S, std::alloc::Global);
    let mut rng = Rng(0x375839675189);
    let vals = (0..S).map(|_| (rng.next() % S as u64) as u16).collect::<Vec<_>>();
    for i in 0..32768 {
        unsafe { palette.set(i, vals[i % S]); }
    }

    b.iter(|| {
        for i in black_box(0..32768) {
            black_box(unsafe { palette.get(i) });
        }
    });
}
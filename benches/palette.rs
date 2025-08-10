
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
    
    c.bench_function("palette-set", |b| b.iter(|| {
        for i in black_box(0..32768) {
            black_box(palette.replace(i, vals[i & 127]));
        }
    }))
    .bench_function("palette-get", |b| b.iter(|| {
        for i in black_box(0..32768) {
            black_box(palette.get(i));
        }
    }));
}

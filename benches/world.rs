
use criterion::*;
use glam::{IVec2, IVec3};
criterion::criterion_group!(benches, benchmarks);
criterion::criterion_main!(benches);
use std::hint::black_box;
use tanuki::{voxel::Voxel, world::{VoxelConfig, VoxelWorld}};

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
    c.bench_function("world-get-set", |b| bench_get_set(b));
}

fn bench_get_set(bencher: &mut Bencher) {
    let mut rng = Rng(8393878631);
    let mut world = VoxelWorld::new(
        VoxelConfig {
            max_y: 320,
            min_y: -64,
        }
    );

    for i in -4..=4 {
        for j in -4..=4 {
            world.init_and_insert_region(IVec2::new(i * 512, j * 512));
        }
    }

    let mut points = Vec::new();
    for _ in 0..4096 {
        let r = rng.next();
        points.push(IVec3 {
            y: (r % 384) as i32 - 64,
            x: ((r >> 16) % 1536) as i32 - 512,
            z: ((r >> 32) % 1536) as i32 - 512,
        });
    }

    bencher.iter(|| {
        for i in 0..4096 {
            let pos = points[i];
            world.set_voxel(pos, Voxel(i as u16));
            black_box(world.get_voxel(pos));
        }
    });
}
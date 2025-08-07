use std::ptr::NonNull;

use bevy_math::IVec2;

use crate::{access::{Cache, VoxelRead, VoxelWrite}, region::Region, world::VoxelWorld};


pub struct VoxelWriter<'w> {
    world: &'w mut VoxelWorld,
    cache: Cache,
}

impl<'w> From<&'w mut VoxelWorld> for VoxelWriter<'w> {
    fn from(value: &'w mut VoxelWorld) -> Self {
        Self {
            world: value,
            cache: Cache::new(),
        }
    }
}

impl<'w> VoxelRead for VoxelWriter<'w> {
    unsafe fn get_region_ptr(&self, xz: IVec2) -> Option<NonNull<Region>> {
        self.cache.search(xz, self.world)
    }
}

impl<'w> VoxelWrite for VoxelWriter<'w> {}

#[cfg(test)]
mod tests {
    use bevy_math::{IVec2, IVec3};

    use crate::{access::{VoxelRead, VoxelWrite}, region::Voxel, tests::TestRng, world::{VoxelConfig, VoxelWorld}};

    #[test]
    fn writer_get_set() {
        let mut world = VoxelWorld::new(
            VoxelConfig {
                max_y: 320,
                min_y: -64,
            }
        );

        world.init_region(IVec2::new(0, 0));
        world.init_region(IVec2::new(-1, -1));
        world.init_region(IVec2::new(-1, 0));
        world.init_region(IVec2::new(0, -1));

        let mut writer = world.writer();
        let mut rng = TestRng::new(0x376783987391);
        for _ in 0..4096 {
            let pt = IVec3 {
                x: (rng.next() & 1023) as i32 - 512,
                z: (rng.next() & 1023) as i32 - 512,
                y: (rng.next() % 384) as i32 - 64,
            };
            let v = Voxel((rng.next() & 127) as u16);

            writer.set_voxel(pt, v);
            assert_eq!(writer.get_voxel(pt), Some(v));
        }
    }
}
use std::ptr::NonNull;

use bevy_math::IVec2;

use crate::{access::{Cache, VoxelRead}, region::Region, world::VoxelWorld};

#[derive(Clone)]
pub struct VoxelReader<'w> {
    world: &'w VoxelWorld,
    cache: Cache,
}

impl<'w> VoxelRead for VoxelReader<'w> {
    unsafe fn get_region_ptr(&self, xz: IVec2) -> Option<NonNull<Region>> {
        self.cache.search(xz, self.world)
    }
}

impl<'w> From<&'w VoxelWorld> for VoxelReader<'w> {
    fn from(value: &'w VoxelWorld) -> Self {
        Self {
            world: value,
            cache: Cache::new(),
        }
    }
}


use std::{collections::BTreeMap, ptr::NonNull};

use bevy_ecs::resource::Resource;
use bevy_math::{IVec2, IVec3};
use generational_arena::{Arena, Index};

use crate::{access::{VoxelRead, VoxelReader, VoxelWriter}, consts::SUBCHUNK_WIDTH, region::Region};

/// Configuration for a VoxelWorld.
#[derive(Clone)]
pub struct VoxelConfig {
    /// The y value above which is "void" space.
    /// Must be greater than min_y
    pub max_y: i32,

    /// The y value below which is "void" space.
    /// Must be less than max_y. 
    pub min_y: i32,
}

#[derive(Resource)]
pub struct VoxelWorld {
    /// The shape and behavior of the VoxelWorld
    config: VoxelConfig,

    /// All Regions in the World
    regions: Arena<NonNull<Region>>,

    /// Map of region origins to region indices.
    lookup: BTreeMap<i64, Index>,
}

impl VoxelWorld {
    pub fn new(config: VoxelConfig) -> Self {
        assert!(config.max_y > config.min_y, "VoxelWorld's max height must be greater than the min height.");
        let height = (config.max_y - config.min_y) as usize;
        assert!(height.is_multiple_of(SUBCHUNK_WIDTH), "The Height of a VoxelWorld must be a multiple of {}", SUBCHUNK_WIDTH);

        Self {
            config,
            regions: Arena::new(),
            lookup: BTreeMap::new(),
        }
    }

    pub fn reader<'w>(&'w self) -> VoxelReader<'w> {
        VoxelReader::from(self)
    }

    pub fn writer<'w>(&'w mut self) -> VoxelWriter<'w> {
        VoxelWriter::from(self)
    }

    pub fn init_region(&mut self, pos: IVec2) -> bool {
        let min = IVec3 {
            x: pos.x &! 511,
            z: pos.y &! 511,
            y: self.config.min_y,
        };

        let max = IVec3 {
            x: min.x + 512,
            z: min.y + 512,
            y: self.config.max_y,
        };

        let key = (min.x >> 9) as i64 | (((min.z >> 9) as i64) << 32);
        if self.lookup.contains_key(&key) {
            return false;
        }

        let region = Region::new(min, max);

        let index = self.regions.insert(region);
        self.lookup.insert(key, index);

        true
    }

    #[inline(always)]
    pub(crate) unsafe fn get_region_ptr_by_key(&self, key: i64) -> Option<NonNull<Region>> {
        Some(*self.regions.get(*self.lookup.get(&key)?).unwrap())
    }
}

impl Drop for VoxelWorld {
    fn drop(&mut self) {
        for (_, ptr) in self.regions.iter_mut() {
            Region::free(*ptr)
        }
    }
}

impl VoxelRead for VoxelWorld {
    #[inline(always)]
    unsafe fn get_region_ptr(&self, xz: IVec2) -> Option<NonNull<Region>> {
        unsafe { self.get_region_ptr_by_key((xz.x >> 9) as i64 | (((xz.y >> 9) as i64) << 32)) }
    }
}

unsafe impl Send for VoxelWorld {}
unsafe impl Sync for VoxelWorld {}


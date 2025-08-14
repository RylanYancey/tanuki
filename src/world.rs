
use glam::{IVec2, IVec3, Vec3Swizzles};
use fxhash::FxHashMap;

use crate::{region::Region, map::Regions, voxel::{Voxel, VoxelIndex, VoxelIndexMut}};

/// Configuration for a VoxelWorld.
#[derive(Clone)]
pub struct VoxelConfig {
    /// The y value above or at which is "void" space.
    /// Must be greater than min_y and a multiple of 32.
    pub max_y: i32,

    /// The y value below which is "void" space.
    /// Must be less than max_y and a multiple of 32.
    pub min_y: i32,
}

pub struct VoxelWorld {
    /// The shape and behavior of the VoxelWorld
    config: VoxelConfig,

    /// The number of voxels tall the world is.
    height: usize,

    /// Map of Region origins to Region Pointers
    regions: Regions,
}

impl VoxelWorld {
    pub fn new(config: VoxelConfig) -> Self {
        assert!(config.max_y > config.min_y, "VoxelWorld's max height must be greater than the min height.");
        let height = (config.max_y - config.min_y) as usize;
        assert!(height.is_multiple_of(32), "The Height of a VoxelWorld must be a multiple of 32");
        Self {
            config,
            height,
            regions: Regions::default(),
        }
    }

    #[inline(always)]
    pub fn min_y(&self) -> i32 {
        self.config.min_y
    }

    #[inline(always)]
    pub fn max_y(&self) -> i32 {
        self.config.max_y
    }

    #[inline(always)]
    pub fn height(&self) -> usize {
        self.height
    }

    /// Insert a Region into the World, returning the existing region if it exists.
    pub fn insert(&mut self, region: Box<Region>) -> Option<Box<Region>> {
        assert!(region.min().y == self.config.min_y && region.max().y == self.config.max_y);
        self.regions.insert(region)
    }

    /// Remove the region that contains the XZ coordinate, if it exists.
    pub fn remove(&mut self, pos: IVec2) -> Option<Box<Region>> {
        self.regions.remove(pos & !511)
    }

    /// Check if a region exists that contains this xz coordiante.
    pub fn has_region(&self, pos: IVec2) -> bool {
        self.regions.has_region(pos & !511)
    }

    /// Initialize a new region containing this position using this World's config.
    pub fn init_region(&mut self, pos: IVec2) -> Box<Region> {
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

        Region::new(min, max)
    }

    /// Initialize a new region and insert it into the world. 
    /// Returns "false" if the region already exists in the world.
    pub fn init_and_insert_region(&mut self, pos: IVec2) -> bool {
        let key = pos &! 511;
        if !self.regions.has_region(key) {
            let region = self.init_region(pos);
            self.regions.insert(region);
            true
        } else {
            false
        }
    }

    /// Get the Region that contains this XZ Position, if it exists.
    #[inline]
    pub fn get_region(&self, pos: IVec2) -> Option<&Region> {
        self.regions.get(pos & !511)
    }

    /// Get the Region that contains this XZ Position, if it exists.
    #[inline]
    pub fn get_region_mut(&mut self, pos: IVec2) -> Option<&mut Region> {
        self.regions.get_mut(pos & !511)
    }

    pub(crate) fn regions(&self) -> &Regions {
        &self.regions
    }

    pub(crate) fn regions_mut(&mut self) -> &mut Regions {
        &mut self.regions
    }

    /// Get the voxel at this position.
    /// Returns "Voxel::AIR" if the position is out-of-bounds.
    #[inline]
    pub fn get_voxel(&self, pos: IVec3) -> Voxel {
        if let Some(i) = VoxelIndex::of(pos, self) {
            i.get_voxel()
        } else {
            Voxel::AIR
        }
    }

    /// Assign to the Voxel at this position, returning the previous value.
    /// Returns "None" if the position is out-of-bounds.
    #[inline]
    pub fn replace_voxel(&mut self, pos: IVec3, voxel: Voxel) -> Option<Voxel> {
        if let Some(mut i) = VoxelIndexMut::of(pos, self) {
            Some(i.replace_voxel(voxel))
        } else {
            None
        }
    }

    /// Assign to the voxel at this position. 
    /// Returns "false" if the position is out of bounds and nothing occurred.
    #[inline(never)]
    pub fn set_voxel(&mut self, pos: IVec3, voxel: Voxel) -> bool {
        if let Some(mut i) = VoxelIndexMut::of(pos, self) {
            i.set_voxel(voxel);
            true
        } else {
            false
        }
    }
}


#[cfg(test)]
mod tests {
    use glam::{IVec2, IVec3};

    use crate::{tests::TestRng, voxel::Voxel, world::{VoxelConfig, VoxelWorld}};

    #[test]
    fn world_get_set_3x3() {
        let mut rng = TestRng::new(3998394589);
        let mut world = VoxelWorld::new(VoxelConfig { 
            max_y: 320,
            min_y: -64
        });

        for i in -1..2 {
            for j in -1..2 {
                world.init_and_insert_region(IVec2::new(i * 512, j * 512));
            }
        }

        for i in 0..16384 {
            let x = (rng.next() % 1536) as i32 - 512;
            let y = (rng.next() % 384) as i32 - 64;
            let z = (rng.next() % 1536) as i32 - 512;
            let v = IVec3::new(x, y, z);

            world.set_voxel(v, Voxel(i));
            assert_eq!(world.get_voxel(v), Voxel(i));
        }
    }
}
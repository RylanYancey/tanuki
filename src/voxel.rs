
use glam::{IVec2, IVec3, Vec3Swizzles};

use crate::{lightmap::Light, region::Region, world::VoxelWorld};


#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Voxel(pub u16);

impl Voxel {
    /// Voxel Zero is reserved for "empty" or "null" state.
    pub const AIR: Self = Self(0);
}

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VoxelData {
    pub state: Voxel,
    pub light: Light,
}

/// Helper struct for computing the indices and origins for accessing voxel data.
#[derive(Copy, Clone)]
pub struct VoxelIndex<'w> {
    pub(crate) region: &'w Region,
    pub(crate) subchunk: usize,
    pub(crate) voxel: usize,
}

impl<'w> VoxelIndex<'w> {
    /// Compute the path to a voxel at this position in this world, if it is in-bounds.
    #[inline(always)]
    pub fn of(pos: IVec3, world: &'w VoxelWorld) -> Option<Self> {
        // convert y value to offset relative to y=0 and bounds check.
        // If y is below the min_y, it will be a very large number because of the cast to usize.
        // If y is above or eq the max_y, oy will be greater than or eq the height.
        let oy = pos.y.wrapping_sub(world.min_y()) as usize;
        if oy >= world.height() { return None }

        // compute origin by rounding down to a multiple of 512, then lookup.
        let origin = pos.xz() & !511;
        let region = world.regions().get(origin)?;

        // convert xz positions to offsets relative to region origin.
        // At this point, pos.xz is known to be within the region (cuz lookup), 
        // so we don't have to do any bounds checking. 
        let ox = (pos.x - origin.x) as usize;
        let oz = (pos.z - origin.y) as usize;

        // Compute index of containing subchunk.
        // This translates to: (x / 32) + ((z / 32) * 16) + ((y / 32) * 256)
        // Regions are in XZY memory order, and have a width/depth of 16 subchunks.
        // The height of the region is variable, thats why it has to be the last dimension,
        // otherwise we would have to `imul` once or twice here.
        let subchunk = (ox >> 5) | ((oz >> 5) << 4) | ((oy >> 5) << 8);

        // Compute index of voxel within containing subchunk.
        // This translates to: (x % 32) + ((z % 32) * 32) + ((z % 32) * 1024)
        // Subchunks have a size of 32x32x32, and have YXZ memory order.
        let voxel = (oy & 31) | ((ox & 31) << 5) | ((oz & 31) << 10);

        Some(Self { region, subchunk, voxel })
    }

    #[inline]
    pub fn get_voxel(&self) -> Voxel {
        Voxel(unsafe { self.region.get_palette_unchecked(self.subchunk).get(self.voxel) })
    }
}

/// Helper struct for computing the indices and origins for accessing voxel data.
pub struct VoxelIndexMut<'w> {
    pub(crate) region: &'w mut Region,
    pub(crate) subchunk: usize,
    pub(crate) voxel: usize,
}

impl<'w> VoxelIndexMut<'w> {
    /// Compute the path to a voxel at this position in this world, if it is in-bounds.
    #[inline(always)]
    pub fn of(pos: IVec3, world: &'w mut VoxelWorld) -> Option<Self> {
        // convert y value to offset relative to y=0 and bounds check.
        // If y is below the min_y, it will be a very large number because of the cast to usize.
        // If y is above or eq the max_y, oy will be greater than or eq the height.
        let oy = pos.y.wrapping_sub(world.min_y()) as usize;
        if oy >= world.height() { return None }

        // compute origin by masking out first 9 bits
        let origin = pos.xz() & !511;
        let region = world.regions_mut().get_mut(origin)?;

        // convert xz positions to offsets relative to region origin.
        // At this point, pos.xz is known to be within the region (cuz lookup), 
        // so we don't have to do any bounds checking. 
        let ox = (pos.x - origin.x) as usize;
        let oz = (pos.z - origin.y) as usize;

        // Compute index of containing subchunk.
        // This translates to: (x / 32) + ((z / 32) * 16) + ((y / 32) * 256)
        // Regions are in XZY memory order, and have a width/depth of 16 subchunks.
        // The height of the region is variable, thats why it has to be the last dimension,
        // otherwise we would have to `imul` once or twice here.
        let subchunk = (ox >> 5) | ((oz >> 5) << 4) | ((oy >> 5) << 8);

        // Compute index of voxel within containing subchunk.
        // This translates to: (x % 32) + ((z % 32) * 32) + ((z % 32) * 1024)
        // Subchunks have a size of 32x32x32, and have YXZ memory order.
        let voxel = (oy & 31) | ((ox & 31) << 5) | ((oz & 31) << 10);

        Some(Self { region, subchunk, voxel })
    }

    #[inline]
    pub fn get_voxel(&self) -> Voxel {
        Voxel(unsafe { self.region.get_palette_unchecked(self.subchunk).get(self.voxel) })
    }

    #[inline]
    pub fn set_voxel(&mut self, voxel: Voxel) {
        unsafe { self.region.get_palette_mut_unchecked(self.subchunk).set(self.voxel, voxel.0) }
    }

    #[inline]
    pub fn replace_voxel(&mut self, voxel: Voxel) -> Voxel {
        Voxel(unsafe { self.region.get_palette_mut_unchecked(self.subchunk).replace(self.voxel, voxel.0) })
    }
}

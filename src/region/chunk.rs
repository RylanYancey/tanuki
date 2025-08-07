use std::{ops::{Deref, DerefMut}, ptr::NonNull};

use bevy_math::IVec3;

use crate::consts::CHUNKS_PER_REGION_SHF;

use super::*;

/// A 32xHx32 Volume of Voxels, or a Column of Subchunks.
pub struct Chunk {
    /// Pointer to the lowest Subchunk in this Chunk's Palette
    palettes: NonNull<PaletteArray<Alloc>>,

    /// Pointer to the lowest Subchunk in this chunk's Lightmap
    lightmaps: NonNull<LightMap<Alloc>>,

    /// Chunk Metadata
    meta: ChunkMeta,

    /// The minimum coordinate contained by this chunk.
    min: IVec3,

    /// The max coordinate contained by this chunk, exclusively.
    max: IVec3,

    /// The number of subchunks in the chunk.
    len: usize,

    /// Index of the lowest subchunk in the chunk.
    index: usize,

    /// Reference to the owning Region.
    /// Yes, this is unsafe, but necessary so we
    /// can update the `set_evs` buffer.
    region: NonNull<Region>,
}

impl Chunk {
    pub fn new(region: NonNull<Region>, index: usize, min: IVec3, max: IVec3) -> Self {
        unsafe {
            Self {
                palettes: region.as_ref().palettes.add(index),
                lightmaps: region.as_ref().lightmaps.add(index),
                meta: ChunkMeta::default(),
                min,
                max,
                len: region.as_ref().shape.chunk_len,
                index,
                region
            }
        }
    }

    pub fn min(&self) -> &IVec3 {
        &self.min
    }

    pub fn max(&self) -> &IVec3 {
        &self.max
    }

    pub fn get_subchunk<'s>(&'s self, y: i32) -> Option<Subchunk<'s>> {
        let oy = (y - self.min.y) >> SUBCHUNK_WIDTH_SHF;
        (oy >= 0 && oy < self.len as i32).then(|| {
            Subchunk {
                region: unsafe { self.region.as_ref() },
                index: self.index + ((oy as usize) << CHUNKS_PER_REGION_SHF),
            }
        })
    }

    pub fn get_subchunk_mut<'s>(&'s mut self, y: i32) -> Option<SubchunkMut<'s>> {
        let oy = (y - self.min.y) >> SUBCHUNK_WIDTH_SHF;
        (oy >= 0 && oy < self.len as i32).then(|| {
            SubchunkMut {
                region: unsafe { self.region.as_mut() },
                index: self.index + ((oy as usize) << CHUNKS_PER_REGION_SHF),
            }
        })
    }

    pub fn get_voxel(&self, pos: IVec3) -> Option<Voxel> {
        let y = pos.y - self.min.y;
        let oy = y >> SUBCHUNK_WIDTH_SHF;
        (oy >= 0 && oy < self.len as i32).then(|| {
            let j = super::to_voxel_index_wrapping(pos.with_y(y));
            Voxel(unsafe { self.palettes.add((oy as usize) << CHUNKS_PER_REGION_SHF).as_ref().get(j) })
        })
    }

    pub fn set_voxel(&mut self, pos: IVec3, voxel: Voxel) -> Option<Voxel> {
        let y = pos.y - self.min.y;
        let oy = y >> SUBCHUNK_WIDTH_SHF;
        (oy >= 0 && oy < self.len as i32).then(|| {
            let j = super::to_voxel_index_wrapping(pos.with_y(y));
            Voxel(unsafe { self.palettes.add((oy as usize) << CHUNKS_PER_REGION_SHF).as_mut().replace(j, voxel.0) })
        })
    }

    pub fn get_light(&self, pos: IVec3) -> Option<Light> {
        let y = pos.y - self.min.y;
        let oy = y >> SUBCHUNK_WIDTH_SHF;
        (oy >= 0 && oy < self.len as i32).then(|| {
            let j = super::to_voxel_index_wrapping(pos.with_y(y));
            unsafe { self.lightmaps.add((oy as usize) << CHUNKS_PER_REGION_SHF).as_ref().get_unchecked(j) }
        })
    }

    pub fn set_light(&mut self, pos: IVec3, light: Light) -> Option<Light> {
        let y = pos.y - self.min.y;
        let oy = y >> SUBCHUNK_WIDTH_SHF;
        (oy >= 0 && oy < self.len as i32).then(|| {
            let j = super::to_voxel_index_wrapping(pos.with_y(y));
            unsafe { self.lightmaps.add((oy as usize) << CHUNKS_PER_REGION_SHF).as_mut().set_unchecked(j, light) }
        })
    }
}

impl Deref for Chunk {
    type Target = ChunkMeta;

    fn deref(&self) -> &Self::Target {
        &self.meta
    }
}

impl DerefMut for Chunk {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.meta
    }
}

pub struct ChunkMeta {

}

impl Default for ChunkMeta {
    fn default() -> Self {
        Self {}
    }
}

/// A 32x32x32 Volume of Voxels 
pub struct Subchunk<'s> {
    pub(super) region: &'s Region,
    pub(super) index: usize,
}

impl<'s> Subchunk<'s> {
    /// This operation is wrapping, out-of-bounds points will wrap to the other side of the subchunk.
    /// This is faster because there are no bounds checks, but can cause issues if not considered.
    pub fn get_voxel(&self, pos: IVec3) -> Voxel {
        let y = pos.y - self.region.min().y;
        let j = super::to_voxel_index_wrapping(pos.with_y(y));
        Voxel(unsafe { self.region.palettes.add(self.index).as_ref().get(j) })
    }

    /// This operation is wrapping, out-of-bounds points will wrap to the other side of the subchunk.
    /// This is faster because there are no bounds checks, but can cause issues if not considered.
    pub fn get_light(&self, pos: IVec3) -> Light {
        let y = pos.y - self.region.min().y;
        let j = super::to_voxel_index_wrapping(pos.with_y(y));
        unsafe { self.region.lightmaps.add(self.index).as_ref().get_unchecked(j) }
    }
}

impl<'s> Deref for Subchunk<'s> {
    type Target = SubchunkMeta;

    fn deref(&self) -> &Self::Target {
        unsafe { self.region.metas.add(self.index).as_ref() }
    }
}

/// A 32x32x32 Volume of Voxels.
pub struct SubchunkMut<'s> {
    pub(super) region: &'s mut Region,
    pub(super) index: usize,
}

impl<'s> SubchunkMut<'s> {
    pub fn get_voxel(&self, pos: IVec3) -> Voxel {
        let y = pos.y - self.region.min().y;
        let j = super::to_voxel_index_wrapping(pos.with_y(y));
        Voxel(unsafe { self.region.palettes.add(self.index).as_ref().get(j) })
    }

    pub fn set_voxel(&mut self, pos: IVec3, voxel: Voxel) -> Voxel {
        let y = pos.y - self.region.min().y;
        let j = super::to_voxel_index_wrapping(pos.with_y(y));
        Voxel(unsafe { self.region.palettes.add(self.index).as_mut().replace(j, voxel.0) })
    }

    pub fn get_light(&self, pos: IVec3) -> Light {
        let y = pos.y - self.region.min().y;
        let j = super::to_voxel_index_wrapping(pos.with_y(y));
        unsafe { self.region.lightmaps.add(self.index).as_ref().get_unchecked(j) }
    }

    pub fn set_light(&mut self, pos: IVec3, light: Light) -> Light {
        let y = pos.y - self.region.min().y;
        let j = super::to_voxel_index_wrapping(pos.with_y(y));
        unsafe { self.region.lightmaps.add(self.index).as_mut().set_unchecked(j, light) }
    }
}

impl<'s> Deref for SubchunkMut<'s> {
    type Target = SubchunkMeta;

    fn deref(&self) -> &Self::Target {
        unsafe { self.region.metas.add(self.index).as_ref() }
    }
}

impl<'s> DerefMut for SubchunkMut<'s> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        unsafe { self.region.metas.add(self.index).as_mut() }
    }
}

pub struct SubchunkMeta {

}

impl Default for SubchunkMeta {
    fn default() -> Self {
        Self {}
    }
}
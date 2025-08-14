use std::{ops::{Deref, DerefMut}, ptr::NonNull};

use glam::IVec3;

use crate::{alloc::Alloc, consts::*, lightmap::{Light, LightMap}, palette::PaletteArray, region::Region, voxel::Voxel};

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
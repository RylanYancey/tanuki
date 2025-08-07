use std::{cell::Cell, ptr::NonNull};

use bevy_math::{IVec2, IVec3, Vec3Swizzles};

use crate::{region::*, world::VoxelWorld};

mod reader;
pub use reader::VoxelReader;
mod writer;
pub use writer::VoxelWriter;
mod worm;
pub use worm::{VoxelDir, Worm, WormMut};

pub trait VoxelRead {
    /// Used because our writers often need multiple mutable references to different 
    /// regions, and to make some of our parallel iterators posisble. 
    unsafe fn get_region_ptr(&self, xz: IVec2) -> Option<NonNull<Region>>;

    /// Get the region containing the XZ position, if it exists in
    /// the reader and is loaded. 
    fn get_region<'w>(&'w self, xz: IVec2) -> Option<&'w Region> {
        unsafe { self.get_region_ptr(xz).map(|ptr| ptr.as_ref()) }
    }

    fn get_chunk<'w>(&'w self, xz: IVec2) -> Option<&'w Chunk> {
        self.get_region(xz).map(|reg| unsafe { reg.get_chunk_unchecked(xz) })
    }

    fn get_subchunk<'w>(&'w self, pos: IVec3) -> Option<Subchunk<'w>> {
        self.get_region(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(|| 
                unsafe { reg.get_subchunk_unchecked(pos) }
            )
        })
    }

    fn get_voxel(&self, pos: IVec3) -> Option<Voxel> {
        self.get_region(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(|| 
                unsafe { reg.get_voxel_unchecked(pos) }
            )
        })
    }

    fn get_light(&self, pos: IVec3) -> Option<Light> {
        self.get_region(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(||  
                unsafe { reg.get_light_unchecked(pos) }
            )
        })
    }

    fn get_data(&self, pos: IVec3) -> Option<VoxelData> {
        self.get_region(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(|| 
                unsafe { reg.get_data_unchecked(pos) }
            )
        })
    }
}

pub trait VoxelWrite: VoxelRead {
    fn get_region_mut<'w>(&'w mut self, xz: IVec2) -> Option<&'w mut Region> {
        Some(unsafe { self.get_region_ptr(xz)?.as_mut() })
    }

    fn get_chunk_mut<'w>(&'w mut self, xz: IVec2) -> Option<&'w mut Chunk> {
        self.get_region_mut(xz).map(|reg| unsafe { reg.get_chunk_mut_unchecked(xz) })
    }

    fn get_subchunk_mut<'w>(&'w mut self, pos: IVec3) -> Option<SubchunkMut<'w>> {
        self.get_region_mut(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(||
                unsafe { reg.get_subchunk_mut_unchecked(pos) }
            )
        })
    }

    fn set_voxel(&mut self, pos: IVec3, voxel: Voxel) -> Option<Voxel> {
        self.get_region_mut(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(||
                unsafe { reg.set_voxel_unchecked(pos, voxel) }
            )
        })
    }

    fn set_light(&mut self, pos: IVec3, light: Light) -> Option<Light> {
        self.get_region_mut(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(||
                unsafe { reg.set_light_unchecked(pos, light) }
            )
        })
    }

    fn set_data(&mut self, pos: IVec3, data: VoxelData) -> Option<VoxelData> {
        self.get_region_mut(pos.xz()).and_then(|reg| {
            (pos.y < reg.max().y && pos.y >= reg.min().y).then(||
                unsafe { reg.set_data_unchecked(pos, data) }
            )
        })
    }
}

#[derive(Clone)]
struct Cache {
    cache_val: Cell<Option<NonNull<Region>>>,
    cache_key: Cell<IVec2>,
}

impl Cache {
    fn new() -> Self {
        Self {
            cache_val: Cell::new(None),
            cache_key: Cell::new(IVec2::MAX),
        }
    }

    #[inline]
    fn search(&self, mut xz: IVec2, world: &VoxelWorld) -> Option<NonNull<Region>> {
        xz.x >>= 9;
        xz.y >>= 9;

        if self.cache_key.get() != xz {
            let key = xz.x as i64 | ((xz.y as i64) << 32);
            self.cache_val.set(unsafe { world.get_region_ptr_by_key(key) });
            self.cache_key.set(xz);
        }

        self.cache_val.get()
    }
}
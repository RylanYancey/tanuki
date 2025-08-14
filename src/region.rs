
use std::{alloc::{Allocator, Layout}, ptr::NonNull};

use glam::{ivec3, IVec2, IVec3, Vec3Swizzles};

use crate::{alloc::{self, Alloc}, lightmap::{Light, LightMap}, palette::PaletteArray, voxel::{Voxel, VoxelData}};

/// A Region is a 512xHx512 volume of voxels where H is a multiple of 32.
/// Regions can be thought of EITHER as a 3d array of Subchunks, or a 2D array of [`Chunk`]s.
/// 
/// # Memory Layout
/// 
/// Subchunks within Regions are in YXZ layout. This means the subchunks are linear on the Y axis,
/// then the X axis, then the Z axis. 
/// 
/// The width of a Region _in voxels_ is 512; in _chunks_ it is 16. Therefore, only 8 bits are needed
/// to store the index of the first subchunk in a chunk; 4 for x and 4 for z. The Y value is variable,
/// so it needs to be after the X and Z. 
pub struct Region {
    /// Subchunk Voxel Data
    palettes: NonNull<PaletteArray<Alloc>>,

    /// The number of subchunks in the Region
    length: usize,

    /// Inclusive lower bound.
    min: IVec3,

    /// Exclusive upper bind.
    max: IVec3,

    /// Allocator, which may at some point be a bump allocator.
    alloc: Alloc,
}

impl Region {
    pub fn new(min: IVec3, max: IVec3) -> Box<Self> {
        let alloc = alloc::init_allocator();
        let height = max.y - min.y;
        let chunk_len = (height >> 5) as usize;
        let length = 256 * chunk_len;
        unsafe {
            // initialize voxel state buffers
            let palettes = {
                let layout = Layout::array::<PaletteArray<Alloc>>(length).unwrap();
                let ptr = alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<PaletteArray<Alloc>>();
                for i in 0..length {
                    ptr.add(i).write(PaletteArray::empty(alloc.clone()));
                }
                ptr
            };

            Box::new(Self {
                alloc: alloc.clone(),
                palettes,
                length,
                min,
                max
            })
        }
    }

    pub fn max(&self) -> &IVec3 {
        &self.max
    }

    pub fn min(&self) -> &IVec3 {
        &self.min
    }

    pub fn origin(&self) -> IVec2 {
        self.min.xz()
    }

    pub(crate) unsafe fn get_palette_unchecked(&self, i: usize) -> &PaletteArray {
        debug_assert!(i < self.length);
        unsafe { self.palettes.add(i).as_ref() }
    }

    pub(crate) unsafe fn get_palette_mut_unchecked(&mut self, i: usize) -> &mut PaletteArray {
        debug_assert!(i < self.length);
        unsafe { self.palettes.add(i).as_mut() }
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        unsafe {
            // drop subchunks
            for i in 0..self.length {
                self.palettes.add(i).drop_in_place();
            }

            // deallocate palettes
            let layout = Layout::array::<PaletteArray<Alloc>>(self.length).unwrap();
            self.alloc.deallocate(self.palettes.cast::<u8>(), layout);
        }
    }
}

unsafe impl Send for Region {}
unsafe impl Sync for Region {}

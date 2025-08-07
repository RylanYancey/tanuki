
use std::{alloc::{Allocator, Layout}, ptr::NonNull};

use bevy_math::{ivec3, IVec2, IVec3, Vec3Swizzles};

use crate::{consts::{CHUNKS_PER_REGION, REGION_WIDTH_CHUNKS, REGION_WIDTH_CHUNKS_SHF, REGION_WIDTH_SHF, SUBCHUNK_WIDTH, SUBCHUNK_WIDTH_SHF}};


mod palette;
pub use palette::PaletteArray;
mod lightmap;
pub use lightmap::{Light, LightMap};
mod chunk;
pub use chunk::{Chunk, ChunkMeta, Subchunk, SubchunkMut, SubchunkMeta};
mod alloc;
pub use alloc::Alloc;
mod voxel;
pub use voxel::{Voxel, VoxelData};

/// A Region is a 512xHx512 volume of voxels where H is a multiple of 32.
/// Regions can be thought of EITHER as a 3d array of Subchunks, or a 2D array of [`Chunk`]s.
/// 
/// # Memory Layout
/// 
/// Subchunks within Regions are in YXZ layout. This means the subchunks are linear on the Y axis,
/// then the X axis, then the Z axis. 
/// 
/// The width of a Region _in voxels_ is 512; in _chunks_ it is 16. Therefore, only 8 bits are needed
/// to store the index of the first subchunk in a chunk - 4 for x and 4 for z. The Y value is variable,
/// so it needs to be after the X and Z. 
pub struct Region {
    alloc: Alloc,
    palettes: NonNull<PaletteArray<Alloc>>,
    lightmaps: NonNull<LightMap<Alloc>>,
    chunks: NonNull<Chunk>,
    metas: NonNull<SubchunkMeta>,
    shape: Shape,
}

impl Region {
    pub fn new(min: IVec3, max: IVec3) -> NonNull<Self> {
        let alloc = alloc::init_allocator();
        let shape = Shape::new(min, max);
        unsafe {
            // initialize voxel state buffers
            let palettes = {
                let layout = Layout::array::<PaletteArray<Alloc>>(shape.num_subchunks).unwrap();
                let ptr = alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<PaletteArray<Alloc>>();
                for i in 0..shape.num_subchunks {
                    ptr.add(i).write(PaletteArray::empty(alloc.clone()));
                }
                ptr
            };

            // initialize voxel lightmaps
            let lightmaps = {
                let layout = Layout::array::<LightMap<Alloc>>(shape.num_subchunks).unwrap();
                let ptr = alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<LightMap<Alloc>>();
                for i in 0..shape.num_subchunks {
                    ptr.add(i).write(LightMap::uniform_none(alloc.clone()));
                }
                ptr
            };

            // initialize subchunk metadata
            let metas = {
                let layout = Layout::array::<SubchunkMeta>(shape.num_subchunks).unwrap();
                let ptr = alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<SubchunkMeta>();
                for i in 0..shape.num_subchunks {
                    ptr.add(i).write(SubchunkMeta::default());
                }
                ptr
            };

            let chunks = {
                let layout = Layout::array::<Chunk>(CHUNKS_PER_REGION).unwrap();
                alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<Chunk>()
            };

            // initialize Region (chunks is uninit)
            let mut region = Box::into_non_null(
                Box::new(Self {
                    alloc: alloc.clone(),
                    palettes,
                    lightmaps,
                    metas,
                    chunks,
                    shape
                })
            );

            for i in 0..CHUNKS_PER_REGION {
                let min = shape.min + to_subchunk_offs(i);
                let max = min + ivec3(SUBCHUNK_WIDTH as i32, shape.height, SUBCHUNK_WIDTH as i32);
                region.as_mut().chunks.add(i).write(Chunk::new(region, i, min, max));
            }

            region
        }
    }

    pub fn free(ptr: NonNull<Self>) {
        let _ = unsafe { Box::from_non_null(ptr) };
    }

    pub fn max(&self) -> &IVec3 {
        &self.shape.max
    }

    pub fn min(&self) -> &IVec3 {
        &self.shape.min
    }

    pub(crate) unsafe fn get_chunk_unchecked<'s>(&'s self, xz: IVec2) -> &'s Chunk {
        let offs = xz - self.shape.min.xz();
        unsafe { self.chunks.add(to_chunk_index(offs)).as_ref() } 
    }

    pub fn get_chunk<'s>(&'s self, xz: IVec2) -> Option<&'s Chunk> {
        let offs = xz - self.shape.min.xz();
        (((offs.x | offs.y) as u32) < 512)
            .then(|| unsafe { self.chunks.add(to_chunk_index(offs)).as_ref() })
    }

    pub(crate) unsafe fn get_chunk_mut_unchecked<'s>(&'s self, xz: IVec2) -> &'s mut Chunk {
        let offs = xz - self.shape.min.xz();
        unsafe { self.chunks.add(to_chunk_index(offs)).as_mut() }
    }

    pub fn get_chunk_mut<'s>(&'s mut self, xz: IVec2) -> Option<&'s mut Chunk> {
        let offs = xz - self.shape.min.xz();
        (((offs.x | offs.y) as u32) < 512)
            .then(|| unsafe { self.chunks.add(to_chunk_index(offs)).as_mut() })
    }

    pub(crate) unsafe fn get_subchunk_unchecked<'s>(&'s self, pos: IVec3) -> Subchunk<'s> {
        let offs = pos - self.shape.min;
        Subchunk {
            region: self,
            index: (offs.y >> SUBCHUNK_WIDTH_SHF) as usize
        }
    }

    pub fn get_subchunk<'s>(&'s self, pos: IVec3) -> Option<Subchunk<'s>> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            Subchunk {
                region: self,
                index: (offs.y >> SUBCHUNK_WIDTH_SHF) as usize
            }
        })
    }

    pub(crate) unsafe fn get_subchunk_mut_unchecked<'s>(&'s mut self, pos: IVec3) -> SubchunkMut<'s> {
        let offs = pos - self.shape.min;
        SubchunkMut {
            region: self,
            index: (offs.y >> SUBCHUNK_WIDTH_SHF) as usize
        }
    }

    pub fn get_subchunk_mut<'s>(&'s mut self, pos: IVec3) -> Option<SubchunkMut<'s>> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            SubchunkMut {
                region: self,
                index: (offs.y >> SUBCHUNK_WIDTH_SHF) as usize
            }
        })
    }

    pub(crate) unsafe fn get_voxel_unchecked(&self, pos: IVec3) -> Voxel {
        let offs = pos - self.shape.min;
        let i = to_subchunk_index(offs);
        let j = to_voxel_index_wrapping(offs);
        Voxel(unsafe { self.palettes.add(i).as_ref().get(j) })
    }

    pub fn get_voxel(&self, pos: IVec3) -> Option<Voxel> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            let i = to_subchunk_index(offs);
            let j = to_voxel_index_wrapping(offs);
            Voxel(unsafe { self.palettes.add(i).as_ref().get(j) })
        })
    }

    pub(crate) unsafe fn set_voxel_unchecked(&mut self, pos: IVec3, voxel: Voxel) -> Voxel {
        let offs = pos - self.shape.min;
        let i = to_subchunk_index(offs);
        let j = to_voxel_index_wrapping(offs);
        Voxel(unsafe { self.palettes.add(i).as_mut().replace(j, voxel.0) })
    }

    pub fn set_voxel(&mut self, pos: IVec3, voxel: Voxel) -> Option<Voxel> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            let i = to_subchunk_index(offs);
            let j = to_voxel_index_wrapping(offs);
            Voxel(unsafe { self.palettes.add(i).as_mut().replace(j, voxel.0) })
        })
    }

    pub(crate) unsafe fn get_light_unchecked(&self, pos: IVec3) -> Light {
        let offs = pos - self.shape.min;
        let i = to_subchunk_index(offs);
        let j = to_voxel_index_wrapping(offs);
        unsafe { self.lightmaps.add(i).as_ref().get_unchecked(j) }
    }

    pub fn get_light(&self, pos: IVec3) -> Option<Light> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            let i = to_subchunk_index(offs);
            let j = to_voxel_index_wrapping(offs);
            unsafe { self.lightmaps.add(i).as_ref().get_unchecked(j) }
        })
    }

    pub(crate) unsafe fn set_light_unchecked(&mut self, pos: IVec3, light: Light) -> Light {
        let offs = pos - self.shape.min;
        let i = to_subchunk_index(offs);
        let j = to_voxel_index_wrapping(offs);
        unsafe { self.lightmaps.add(i).as_mut().set_unchecked(j, light) }
    }

    pub fn set_light(&mut self, pos: IVec3, light: Light) -> Option<Light> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            let i = to_subchunk_index(offs);
            let j = to_voxel_index_wrapping(offs);
            unsafe { self.lightmaps.add(i).as_mut().set_unchecked(j, light) }
        })
    }

    pub(crate) unsafe fn get_data_unchecked(&self, pos: IVec3) -> VoxelData {
        let offs = pos - self.shape.min;
        let i = to_subchunk_index(offs);
        let j = to_voxel_index_wrapping(offs);
        VoxelData {
            light: unsafe { self.lightmaps.add(i).as_ref().get_unchecked(j) },
            state: Voxel(unsafe { self.palettes.add(i).as_ref().get(j) })
        }
    }

    pub fn get_data(&self, pos: IVec3) -> Option<VoxelData> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            let i = to_subchunk_index(offs);
            let j = to_voxel_index_wrapping(offs);
            VoxelData {
                light: unsafe { self.lightmaps.add(i).as_ref().get_unchecked(j) },
                state: Voxel(unsafe { self.palettes.add(i).as_ref().get(j) })
            }
        })
    }

    pub(crate) unsafe fn set_data_unchecked(&mut self, pos: IVec3, data: VoxelData) -> VoxelData {
        let offs = pos - self.shape.min;
        let i = to_subchunk_index(offs);
        let j = to_voxel_index_wrapping(offs);
        VoxelData {
            light: unsafe { self.lightmaps.add(i).as_mut().set_unchecked(j, data.light) },
            state: Voxel(unsafe { self.palettes.add(i).as_mut().replace(j, data.state.0) })
        }
    }

    pub fn set_data(&mut self, pos: IVec3, data: VoxelData) -> Option<VoxelData> {
        let offs = pos - self.shape.min;
        (((offs.x | offs.y | offs.z) as u32) < 512 && offs.y < self.shape.height).then(|| {
            let i = to_subchunk_index(offs);
            let j = to_voxel_index_wrapping(offs);
            VoxelData {
                light: unsafe { self.lightmaps.add(i).as_mut().set_unchecked(j, data.light) },
                state: Voxel(unsafe { self.palettes.add(i).as_mut().replace(j, data.state.0) })
            }
        })
    }
}

impl Drop for Region {
    fn drop(&mut self) {
        unsafe {
            // drop chunks
            for i in 0..CHUNKS_PER_REGION {
                self.chunks.add(i).drop_in_place();
            }

            // drop subchunks
            for i in 0..self.shape.num_subchunks {
                self.palettes.add(i).drop_in_place();
                self.lightmaps.add(i).drop_in_place();
                self.metas.add(i).drop_in_place();
            }

            // deallocate palettes
            let layout = Layout::array::<PaletteArray<Alloc>>(self.shape.num_subchunks).unwrap();
            self.alloc.deallocate(self.palettes.cast::<u8>(), layout);
            // deallocate lightmaps
            let layout = Layout::array::<LightMap<Alloc>>(self.shape.num_subchunks).unwrap();
            self.alloc.deallocate(self.lightmaps.cast::<u8>(), layout);
            // deallocate metas
            let layout = Layout::array::<SubchunkMeta>(self.shape.num_subchunks).unwrap();
            self.alloc.deallocate(self.metas.cast::<u8>(), layout);
            // deallocate chunks
            let layout = Layout::array::<Chunk>(CHUNKS_PER_REGION).unwrap();
            self.alloc.deallocate(self.chunks.cast::<u8>(), layout);
        }
    }
}

unsafe impl Send for Region {}
unsafe impl Sync for Region {}

#[derive(Copy, Clone)]
pub struct Shape {
    pub height: i32,
    pub chunk_len: usize,
    pub num_subchunks: usize,
    pub min: IVec3,
    pub max: IVec3,
}

impl Shape {
    pub fn new(min: IVec3, max: IVec3) -> Self {
        let height = max.y - min.y;
        let chunk_len = (height >> SUBCHUNK_WIDTH_SHF) as usize;
        Self {
            height,
            chunk_len,
            num_subchunks: CHUNKS_PER_REGION * chunk_len,
            min,
            max
        }
    }
}

/// The number of bits needed to store X and Z
const XZ_BITWIDTH: usize = REGION_WIDTH_CHUNKS_SHF * 2;

/// Get the index of the containing chunk given an xz offset relative to region origin.
/// The XZ are expected to be in the range [0,512)
/// 
/// The first four bits of the returned index encode the X value, and the next four bits
/// encode the Z value.
pub(crate) fn to_chunk_index(xz: IVec2) -> usize {
    ((xz.x >> SUBCHUNK_WIDTH_SHF) | ((xz.y >> SUBCHUNK_WIDTH_SHF) << REGION_WIDTH_CHUNKS_SHF)) as usize
}

/// Get the XZ offset of a chunk, given the index of the lowest subchunk it contains.
pub(crate) fn to_chunk_offs(i: usize) -> IVec2 {
    IVec2 {
        x: ((i & (REGION_WIDTH_CHUNKS - 1)) << SUBCHUNK_WIDTH_SHF) as i32,
        y: (((i >> REGION_WIDTH_CHUNKS_SHF) & (REGION_WIDTH_CHUNKS - 1)) << SUBCHUNK_WIDTH_SHF) as i32
    }
}

/// Get the index of a subchunk relative to 0,0,0.
/// The XZ components of the input must be in the range [0,512)
/// The Y component must be in the range [0,height)
/// 
/// The first 4 bits are X, the next 4 are Z, and the remaining are Y.
pub(crate) fn to_subchunk_index(offs: IVec3) -> usize {
    to_chunk_index(offs.xz()) | ((offs.y >> SUBCHUNK_WIDTH_SHF) << XZ_BITWIDTH) as usize
}

/// Get the XYZ offset of a subchunk, given its index.
/// The returned point is relative to 0,0,0.
/// The returned XZ are in the range [0,512).
/// The returned Y is in the range [0,height)
pub(crate) fn to_subchunk_offs(i: usize) -> IVec3 {
    IVec3 {
        x: ((i & (REGION_WIDTH_CHUNKS - 1)) << SUBCHUNK_WIDTH_SHF) as i32,
        z: (((i >> REGION_WIDTH_CHUNKS_SHF) & (REGION_WIDTH_CHUNKS - 1)) << SUBCHUNK_WIDTH_SHF) as i32,
        y: ((i >> XZ_BITWIDTH) << SUBCHUNK_WIDTH_SHF) as i32,
    }
}

pub(crate) fn to_voxel_index_wrapping(offs: IVec3) -> usize {
    const W: i32 = (SUBCHUNK_WIDTH - 1) as i32;
    (
        (offs.y & W) 
        | ((offs.x & W) << SUBCHUNK_WIDTH_SHF) 
        | ((offs.z & W) << (SUBCHUNK_WIDTH_SHF*2))
    ) as usize
}

#[cfg(test)]
mod tests {
    use bevy_math::{IVec2, IVec3};

    use crate::{consts::{CHUNKS_PER_REGION, SUBCHUNK_LENGTH, SUBCHUNK_WIDTH}, region::{Light, Region, Voxel, VoxelData}, tests::TestRng};

    #[test]
    fn test_index_compute() {
        // to chunk index
        assert_eq!(super::to_chunk_index(IVec2::new(0, 0)), 0);
        assert_eq!(super::to_chunk_index(IVec2::new(511, 511)), CHUNKS_PER_REGION - 1);

        // to voxel index
        assert_eq!(super::to_voxel_index_wrapping(IVec3::new(0, 0, 0)), 0);
        assert_eq!(super::to_voxel_index_wrapping(IVec3::splat(SUBCHUNK_WIDTH as i32 - 1)), SUBCHUNK_LENGTH - 1);
    }

    #[test]
    fn region_get_set() {
        let mut region_ptr = Region::new(IVec3::new(0, -60, 0), IVec3::new(512, 324, 512));
        let region = unsafe { region_ptr.as_mut() };
        let mut rng = TestRng::new(0x39567387819381);

        // assign voxels
        for _ in 0..4096 {
            let pt = IVec3 {
                x: (rng.next() & 511) as i32,
                y: (rng.next() % 384) as i32 - 60,
                z: (rng.next() & 511) as i32,
            };
            let val = Voxel((rng.next() & 127) as u16);
            region.set_voxel(pt, val);
            assert_eq!(Some(val), region.get_voxel(pt), "{:?}", pt);
        }

        // test light get/set
        for _ in 0..4096 {
            let pt = IVec3 {
                x: (rng.next() & 511) as i32,
                y: (rng.next() % 384) as i32 - 60,
                z: (rng.next() & 511) as i32,
            };
            let val = Light { intensity: (rng.next() & 255) as u8, hsl_color: 0 };
            region.set_light(pt, val);
            assert_eq!(Some(val), region.get_light(pt), "{:?}", pt);
        }

        // test data get/set
        for _ in 0..4096 {
            let pt = IVec3 {
                x: (rng.next() & 511) as i32,
                y: (rng.next() % 384) as i32 - 60,
                z: (rng.next() & 511) as i32,
            };

            let data = VoxelData {
                light: Light { intensity: (rng.next() & 255) as u8, hsl_color: 0 },
                state: Voxel((rng.next() & 127) as u16),
            };

            region.set_data(pt, data);
            assert_eq!(Some(data), region.get_data(pt), "{:?}", pt);
        }

        // y above bounds
        for i in 0..16 {
            let pt = IVec3 {
                x: (rng.next() & 511) as i32,
                y: (rng.next() % 384) as i32 + 324,
                z: (rng.next() & 511) as i32,
            };
            assert_eq!(region.get_voxel(pt), None);
            assert_eq!(region.set_voxel(pt, Voxel(i as u16)), None);
        }

        // x above bounds
        for i in 0..16 {
            let pt = IVec3 {
                x: (rng.next() & 511) as i32 + 512,
                y: (rng.next() % 384) as i32 - 60,
                z: (rng.next() & 511) as i32,
            };
            assert_eq!(region.get_voxel(pt), None);
            assert_eq!(region.set_voxel(pt, Voxel(i as u16)), None);
        }

        // z above bounds
        for i in 0..16 {
            let pt = IVec3 {
                x: (rng.next() & 511) as i32,
                y: (rng.next() % 384) as i32 - 60,
                z: (rng.next() & 511) as i32 + 512,
            };
            assert_eq!(region.get_voxel(pt), None);
            assert_eq!(region.set_voxel(pt, Voxel(i as u16)), None);
        }

        Region::free(region_ptr)
    }
}
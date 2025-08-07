
use std::ptr::NonNull;

use bevy_math::{ivec3, IVec3, Vec3Swizzles};

use crate::{access::{VoxelRead, VoxelWrite}, consts::*, region::Region};

#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Debug, Hash)]
#[repr(i8)]
pub enum VoxelDir {
    Up = 1,
    Down = -1,
    East = 2,
    West = -2,
    North = 3,
    South = -3,
}

impl VoxelDir {
    /// All voxel directions packed into an array for easy iteration.
    pub const ALL: [Self; 6] = [
        Self::Up, Self::Down, Self::East,
        Self::West, Self::North, Self::South,
    ];

    #[inline(always)]
    const unsafe fn from_i8(i: i8) -> Self {
        unsafe { std::mem::transmute(i) }
    }

    /// Get the direction opposite this one.
    /// e.g. VoxelDir::East => VoxelDir::West
    #[inline]
    pub const fn invert(self) -> Self {
        unsafe { Self::from_i8((self as i8) * -1) }
    }

    /// If the direction is negative, invert it. 
    #[inline]
    pub const fn abs(self) -> Self {
        unsafe { Self::from_i8(i8::abs(self as i8))}
    }

    /// Whether this direction is negative or positive.
    #[inline]
    pub const fn sign(self) -> i8 {
        i8::signum(self as i8)
    }

    /// Get the direction expressed as a unit vector.
    #[inline]
    pub const fn as_ivec3(self) -> IVec3 {
        match self {
            Self::Up => ivec3(0, 1, 0),
            Self::Down => ivec3(0, -1, 0),
            Self::East => ivec3(1, 0, 0),
            Self::West => ivec3(-1, 0, 0),
            Self::North => ivec3(0, 0, 1),
            Self::South => ivec3(0, 0, -1),
        }
    }

    /// Extract the relevant axis component from this vector.
    #[inline]
    pub const fn extract_axis(self, vec: IVec3) -> i32 {
        match self.abs() {
            Self::Up => vec.y,
            Self::East => vec.x,
            _ => vec.z,
        }
    }

    /// Get the next position in this direction.
    #[inline]
    pub const fn next(self, pos: IVec3) -> IVec3 {
        let vec = self.as_ivec3();
        IVec3 {
            x: pos.x + vec.x,
            y: pos.y + vec.y,
            z: pos.z + vec.z,
        }   
    }
}

#[derive(Copy, Clone)]
pub struct Worm<'w, R: VoxelRead> {
    reader: &'w R,
    data: WormData,
}

impl<'w, R: VoxelRead> Worm<'w, R> {
    pub fn new(reader: &'w R, pos: IVec3) -> Option<Self> {
        unsafe {
            reader.get_region_ptr(pos.xz()).and_then(|ptr| {
                let refr = ptr.as_ref();
                if pos.y < refr.min().y || pos.y >= refr.max().y {
                    None
                } else {
                    Some(Self {
                        reader,
                        data: WormData::new(pos, ptr)
                    })
                }
            })
        }
    }

    pub fn next(&self, dir: VoxelDir) -> Option<Self> {
        match dir {
            VoxelDir::Up => self.data.up(self.reader),
            VoxelDir::Down => self.data.down(self.reader),
            VoxelDir::East => self.data.east(self.reader),
            VoxelDir::West => self.data.west(self.reader),
            VoxelDir::North => self.data.north(self.reader),
            VoxelDir::South => self.data.south(self.reader),
        }.map(|data| {
            Self {
                reader: self.reader,
                data
            }
        })
    }
}

pub struct WormMut<'w, W: VoxelWrite> {
    reader: &'w W,
    data: WormData,
}

impl<'w, W: VoxelWrite> WormMut<'w, W> {
    pub fn new(reader: &'w W, pos: IVec3) -> Option<Self> {
        unsafe {
            reader.get_region_ptr(pos.xz()).and_then(|ptr| {
                let refr = ptr.as_ref();
                if pos.y < refr.min().y || pos.y >= refr.max().y {
                    None
                } else {
                    Some(Self {
                        reader,
                        data: WormData::new(pos, ptr)
                    })
                }
            })
        }
    }

    pub fn next(&self, dir: VoxelDir) -> Option<Self> {
        match dir {
            VoxelDir::Up => self.data.up(self.reader),
            VoxelDir::Down => self.data.down(self.reader),
            VoxelDir::East => self.data.east(self.reader),
            VoxelDir::West => self.data.west(self.reader),
            VoxelDir::North => self.data.north(self.reader),
            VoxelDir::South => self.data.south(self.reader),
        }.map(|data| {
            Self {
                reader: self.reader,
                data
            }
        })
    }
}

#[derive(Copy, Clone)]
struct WormData {
    region: NonNull<Region>,
    subchunk: usize,
    voxel: usize,
    pos: IVec3,
}

impl WormData {
    fn new(pos: IVec3, region: NonNull<Region>) -> Self {
        let offs = unsafe { pos - region.as_ref().min() };
        Self {
            region,
            subchunk: crate::region::to_subchunk_index(offs),
            voxel: crate::region::to_voxel_index_wrapping(offs),
            pos,
        }
    }

    #[inline]
    fn next(
        &self,
        reader: Option<&impl VoxelRead>,
        pos: IVec3,
        is_subchunk_edge: bool,
        is_region_edge: bool,
        deltas: &Deltas,
    ) -> Option<Self> {
        // subchunk index doesn't change if no boundaries are crossed.
        let mut subch_delta = 0;
        // voxel delta always changes by at least voxel_next
        let mut voxel_delta = deltas.voxel_next;
        // region may be changed if a region boundary is crossed and reader is Some.
        let mut region = self.region;

        // if we are on a subchunk edge, the voxel index needs to 
        // wrap to the other side of the subchunk.
        if is_subchunk_edge {
            voxel_delta = deltas.voxel_wrap;
            if is_region_edge {
                // if we are on a region edge, the subchunk index needs to wrap to 
                // the other side of the region, and the new region needs to be fetched.
                region = unsafe { reader?.get_region_ptr(pos.xz())? };
                subch_delta = deltas.subch_wrap;
            } else {
                // if we are not on a region edge, but ARE on a subchunk edge, 
                // the subchunk index needs to change because a boundary has been crossed.
                subch_delta = deltas.subch_next;
            }
        } 

        Some(Self {
            pos,
            region,
            subchunk: (self.subchunk as isize + subch_delta) as usize,
            voxel: (self.voxel as isize + voxel_delta) as usize,
        })
    }

    #[inline]
    fn up<R: VoxelRead>(&self, _: &R) -> Option<Self> {
        self.next(
            None::<&R>,
            self.pos.with_y(self.pos.y + 1),
            self.pos.y & SUBCHUNK_WIDTH_WRAP as i32 == SUBCHUNK_WIDTH_WRAP as i32,
            self.pos.y >= unsafe { self.region.as_ref() }.max().y - 1,
            &Deltas::UP
        )
    }

    #[inline]
    fn down<R: VoxelRead>(&self, _: &R) -> Option<Self> {
        self.next(
            None::<&R>,
            self.pos.with_y(self.pos.y - 1),
            self.pos.y & SUBCHUNK_WIDTH_WRAP as i32 == 0,
            self.pos.y <= unsafe { self.region.as_ref() }.min().y,
            &Deltas::DOWN
        )
    }

    #[inline]
    fn east(&self, read: &impl VoxelRead) -> Option<Self> {
        self.next(
            Some(read),
            self.pos.with_x(self.pos.x + 1),
            self.pos.x & SUBCHUNK_WIDTH_WRAP as i32 == SUBCHUNK_WIDTH_WRAP as i32,
            self.pos.x & 511 == 511,
            &Deltas::EAST
        )
    }

    #[inline]
    fn west(&self, read: &impl VoxelRead) -> Option<Self> {
        self.next(
            Some(read),
            self.pos.with_x(self.pos.x - 1),
            self.pos.x & SUBCHUNK_WIDTH_WRAP as i32 == 0,
            self.pos.x & 511 == 0,
            &Deltas::WEST
        )
    }

    #[inline]
    fn north(&self, read: &impl VoxelRead) -> Option<Self> {
        self.next(
            Some(read),
            self.pos.with_z(self.pos.z + 1),
            self.pos.z & SUBCHUNK_WIDTH_WRAP as i32 == SUBCHUNK_WIDTH_WRAP as i32,
            self.pos.z & 511 == 511,
            &Deltas::NORTH
        )
    }

    #[inline]
    fn south(&self, read: &impl VoxelRead) -> Option<Self> {
        self.next(
            Some(read),
            self.pos.with_z(self.pos.z - 1),
            self.pos.z & SUBCHUNK_WIDTH_WRAP as i32 == 0,
            self.pos.z & 511 == 0,
            &Deltas::SOUTH
        )
    }
}

struct Deltas {
    voxel_wrap: isize,
    voxel_next: isize,
    subch_wrap: isize,
    subch_next: isize,
}

impl Deltas {
    const UP: Self = Self {
        voxel_wrap: -VOXEL_Y_WRAP_DELTA,
        voxel_next: VOXEL_Y_NEXT_DELTA,
        subch_wrap: -SUBCH_Y_WRAP_DELTA,
        subch_next: SUBCH_Y_NEXT_DELTA,
    };
    const DOWN: Self = Self {
        voxel_wrap: VOXEL_Y_WRAP_DELTA,
        voxel_next: -VOXEL_Y_NEXT_DELTA,
        subch_wrap: SUBCH_Y_WRAP_DELTA,
        subch_next: -SUBCH_Y_NEXT_DELTA,
    };
    const EAST: Self = Self {
        voxel_wrap: -VOXEL_X_WRAP_DELTA,
        voxel_next: VOXEL_X_NEXT_DELTA,
        subch_wrap: -SUBCH_X_WRAP_DELTA,
        subch_next: SUBCH_X_NEXT_DELTA,
    };
    const WEST: Self = Self {
        voxel_wrap: VOXEL_X_WRAP_DELTA,
        voxel_next: -VOXEL_X_NEXT_DELTA,
        subch_wrap: SUBCH_X_WRAP_DELTA,
        subch_next: -SUBCH_X_NEXT_DELTA
    };
    const NORTH: Self = Self {
        voxel_wrap: -VOXEL_Z_WRAP_DELTA,
        voxel_next: VOXEL_Z_NEXT_DELTA,
        subch_wrap: -SUBCH_Z_WRAP_DELTA,
        subch_next: SUBCH_Z_NEXT_DELTA,
    };
    const SOUTH: Self = Self {
        voxel_wrap: VOXEL_Z_WRAP_DELTA,
        voxel_next: -VOXEL_Z_NEXT_DELTA,
        subch_wrap: SUBCH_Z_WRAP_DELTA,
        subch_next: -SUBCH_Z_NEXT_DELTA,
    };
}

const VOXEL_Y_WRAP_DELTA: isize = SUBCHUNK_WIDTH as isize - 1;
const VOXEL_Y_NEXT_DELTA: isize = 1;
const SUBCH_Y_WRAP_DELTA: isize = 0;
const SUBCH_Y_NEXT_DELTA: isize = CHUNKS_PER_REGION as isize;
const VOXEL_X_WRAP_DELTA: isize = (SUBCHUNK_WIDTH.pow(2) - (SUBCHUNK_WIDTH - 1)) as isize;
const VOXEL_X_NEXT_DELTA: isize = SUBCHUNK_WIDTH as isize;
const SUBCH_X_WRAP_DELTA: isize = (REGION_WIDTH - 1) as isize;
const SUBCH_X_NEXT_DELTA: isize = 1;
const VOXEL_Z_WRAP_DELTA: isize = (SUBCHUNK_WIDTH.pow(3) - (SUBCHUNK_WIDTH.pow(2) - 1)) as isize;
const VOXEL_Z_NEXT_DELTA: isize = SUBCHUNK_WIDTH.pow(2) as isize;
const SUBCH_Z_WRAP_DELTA: isize = (REGION_WIDTH_CHUNKS.pow(2) - (REGION_WIDTH_CHUNKS - 1)) as isize;
const SUBCH_Z_NEXT_DELTA: isize = REGION_WIDTH as isize;
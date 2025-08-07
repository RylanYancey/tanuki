
use crate::region::lightmap::Light;


#[derive(Copy, Clone, Eq, PartialEq, Ord, PartialOrd, Hash, Debug)]
pub struct Voxel(pub u16);

#[derive(Copy, Clone, Debug, Eq, PartialEq)]
pub struct VoxelData {
    pub state: Voxel,
    pub light: Light,
}
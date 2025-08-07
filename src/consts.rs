
pub const SUBCHUNK_WIDTH: usize = 32;
pub const SUBCHUNK_LENGTH: usize = 32768;
pub const SUBCHUNK_WIDTH_SHF: usize = 5;
pub const CHUNKS_PER_REGION: usize = 256;
pub const CHUNKS_PER_REGION_SHF: usize = 16;
pub const REGION_WIDTH: usize = 512;
pub const REGION_WIDTH_SHF: usize = 9;
pub const REGION_WIDTH_CHUNKS: usize = 16;
pub const REGION_WIDTH_CHUNKS_SHF: usize = 4;

#[cfg(target_feature = "avx512f")]
pub const SIMD_LANES: usize = 64; // 512 bits

#[cfg(all(target_feature = "avx2", not(target_feature = "avx512f")))]
pub const SIMD_LANES: usize = 32; // 256 bits

#[cfg(not(any(target_feature = "avx2", target_feature = "avx512f")))]
pub const SIMD_LANES: usize = 16; // 128 bits
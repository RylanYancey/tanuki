
#![feature(allocator_api)]
#![feature(select_unpredictable)]
#![feature(portable_simd)]
#![feature(slice_ptr_get)]
#![feature(box_vec_non_null)]

pub mod consts;
pub mod region;
pub mod access;
pub mod world;

#[cfg(test)]
mod tests {
    pub struct TestRng(pub u64);

    impl TestRng {
        pub fn new(seed: u64) -> Self {
            TestRng(seed)
        }

        pub fn next(&mut self) -> u64 {
            let r = u128::from(self.0).wrapping_mul(0x8373ABCDEF397838ABCDEF1);
            let h = ((r >> 64) ^ r) as u64;
            self.0 = self.0.wrapping_add(h);
            h
        }
    }
}
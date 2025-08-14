
use std::{mem, ptr::NonNull};
use glam::IVec2;

use crate::region::Region;

/// Lookup table for Regions by Origin.
/// 
/// This is implemented with a Perfect Hash Function, 
/// which we can do because of how infrequently regions
/// are inserted/removed. It also makes the binary size of 
/// searching significantly smaller, so it can easily be 
/// inlined into any call site - unlike HashMap, which has 
/// alot of extra instructions for SIMD quadratic probing.
/// 
/// Resolving a region origin to a Region ptr is done like this:
///  - Combine origin XZ into a u64 with the `to_key` function.
///  - Compute the hash by multiplying the key by the magic.
///  - Extract the index from the hash by shifting right until in-range bits remain.
///  - Check if the bucket at that index's key matches the key we just computed, return if true.
pub struct Regions {
    regions: Vec<NonNull<Region>>,
    buckets: Vec<Bucket>,
    shift: u32,
    magic: u64,
    state: u64,
}

impl Regions {
    #[inline(always)]
    pub fn get(&self, origin: IVec2) -> Option<&Region> {
        let key = to_key(origin);
        self.buckets[self.hash(key)].try_get(key)
    }

    #[inline(always)]
    pub fn get_mut(&mut self, origin: IVec2) -> Option<&mut Region> {
        let key = to_key(origin);
        let hash = self.hash(key);
        self.buckets[hash].try_get_mut(key)
    }

    #[inline(always)]
    pub fn has_region(&self, origin: IVec2) -> bool {
        let key = to_key(origin);
        self.buckets[self.hash(key)].key == key
    }

    /// Never rebuilds.
    pub fn remove(&mut self, origin: IVec2) -> Option<Box<Region>> {
        let key = to_key(origin);
        let hash = self.hash(key);
        let bucket = &mut self.buckets[hash];
        if bucket.key == key {
            let idx = bucket.idx;
            self.buckets[hash] = Bucket::EMPTY;
            let ret = self.regions.swap_remove(idx);
            // update the bucket index of the region we just moved from the end, if it exists.
            if let Some(region) = self.regions.get(idx) {
                let key = to_key(unsafe { region.as_ref().origin() });
                let hash = self.hash(key);
                self.buckets[hash].idx = idx;
            }
            Some(unsafe { Box::from_non_null(ret) })
        } else {
            None
        }
    }

    /// Rebuilds if a hash conflict occurs.
    pub fn insert(&mut self, region: Box<Region>) -> Option<Box<Region>> {
        let key = to_key(region.origin());
        let hash = self.hash(key);
        let bucket = &mut self.buckets[hash];
        let ptr = Box::into_non_null(region);

        if bucket.key == key {
            // replace existing if region already exists
            return Some(unsafe { 
                Box::from_non_null(mem::replace(&mut self.regions[bucket.idx], ptr))
            });
        } if bucket.key == u64::MAX {
            // occupy bucket and push
            bucket.ptr = ptr;
            bucket.idx = self.regions.len();
            bucket.key = key;
            self.regions.push(ptr);
        } else {
            // conflict; rebuild
            self.regions.push(ptr);
            self.rebuild();
        }

        None
    }

    /// Compute the hash by multiplying the key (created with to_key) by the computed magic.
    /// Then, shift right such that only the last N bits remain. The shift factor is set such that
    /// after this shift, the value is known to be in-range for the buckets. 
    /// 
    /// Alternatively, we could have masked with &, but because the key always has its first 9 bits
    /// set to 0 we would have to shift anyway, so we're saving time by only shifting instead of &.
    #[inline(always)]
    fn hash(&self, key: u64) -> usize {
        (self.magic.wrapping_mul(key) >> self.shift) as usize
    }

    fn rebuild(&mut self) {
        if self.regions.len() == 0 {
            *self = Self::default();
            return;
        }

        if self.regions.len() == 1 {
            self.buckets.clear();
            self.buckets.push(Bucket {
                ptr: self.regions[0],
                key: to_key(unsafe { self.regions[0].as_ref().origin() }),
                idx: 0,
            });
            self.magic = 0;
            self.shift = 63;
            return;
        }

        // The size of buckets is always at least 50% larger than regions, and is rounded up to a power of two.
        let size = (self.regions.len() + (self.regions.len() >> 1)).next_power_of_two();
        // Shift factor that ensures right shift by this factor is in the range 0..size
        self.shift = 64 - size.trailing_zeros();

        self.buckets.clear();
        self.buckets.resize(size, Bucket::EMPTY);
        
        // indices that have been assigned to by buckets so we don't have to clear the 
        // entire `buckets` vector every time rebuilding fails.
        let mut stack = Vec::new();

        // keeps track of how many iterations it took to build.
        let mut n = 0;

        'outer: loop {
            // compute next magic with a basic WyRand impl
            const P0: u64 = 0xa076_1d64_78bd_642f;
            const P1: u64 = 0xe703_7ed1_a0b4_28db;
            self.state = self.state.wrapping_add(P0);
            let r = u128::from(self.state).wrapping_mul(u128::from(self.state ^ P1));
            self.magic = ((r >> 64) ^ r) as u64;

            n += 1;
            const MAX_RETRIES: usize = 1000;
            if n >= MAX_RETRIES {
                panic!("WorldMap failed to rebuild in {MAX_RETRIES} iterations.");
            }

            for i in 0..self.regions.len() {
                let region = self.regions[i];
                unsafe {
                    let key = to_key(region.as_ref().origin());
                    let hash = self.hash(key);
                    if self.buckets[hash].key == u64::MAX {
                        // bucket untaken; success
                        stack.push(hash);
                        self.buckets[hash] = Bucket {
                            ptr: region,
                            key,
                            idx: i,
                        };
                    } else {
                        // bucket already taken; clear and try again.
                        while let Some(i) = stack.pop() {
                            self.buckets[i].key = u64::MAX;
                        }
                        continue 'outer;
                    }
                }
            }

            return;
        }
    }
}

impl Drop for Regions {
    fn drop(&mut self) {
        while let Some(region) = self.regions.pop() {
            let _ = unsafe { Box::from_non_null(region) };
        }
    }
}

impl Default for Regions {
    fn default() -> Self {
        Self {
            regions: Vec::new(),
            buckets: vec![Bucket::EMPTY],
            shift: 63,
            magic: 0,
            state: 0xda3e_39cb_94b9_5bdb,
        }
    }
}

#[derive(Copy, Clone)]
struct Bucket {
    ptr: NonNull<Region>,
    key: u64,
    idx: usize,
}

impl Bucket {
    const EMPTY: Self = Self { ptr: NonNull::dangling(), key: u64::MAX, idx: 0 };

    #[inline(always)]
    fn try_get(&self, key: u64) -> Option<&Region> {
        self.key.eq(&key).then(|| unsafe { self.ptr.as_ref() })
    }

    #[inline(always)]
    fn try_get_mut(&mut self, key: u64) -> Option<&mut Region> {
        self.key.eq(&key).then(|| unsafe { self.ptr.as_mut() })
    }
}

/// Make the upper 32 bits the X origin, lower 32 bits are the Y origin.
#[inline(always)]
fn to_key(origin: IVec2) -> u64 {
    ((origin.x as u64) << 32) | (origin.y as u32 as u64)
}


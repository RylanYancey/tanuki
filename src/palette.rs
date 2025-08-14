use std::{alloc::{Allocator, Global, Layout}, cell::{OnceCell, RefCell}, ptr::NonNull, simd::prelude::*, time::Duration};

use crate::voxel::Voxel;

static mut BPI_ZERO_WORD: usize = 0;
static mut BPI_ZERO_PALETTE: u16 = 0;

#[repr(align(64))]
pub struct PaletteArray<A: Allocator=Global> {
    /// Storage for indices of voxel states in the palette.
    /// These indices are "packed" according to the bpi, to 
    /// access them we must extract with a series of bitops.
    words: NonNull<usize>,

    /// Set of all Voxel states represented in this array.
    /// The order of the palette must never change, becuase a change 
    /// would invalidate any indices that point to that element.
    /// The first entry in the palette is always 0. 
    palette: NonNull<u16>,
    palette_len: u16,
    palette_cap: u16,

    /// Hashmap of Voxel states for fast lookup.
    /// The items in cache are voxel keys to palette indices.
    /// "cache_bits" is the available capacity minus 1. 
    /// "cache_size" is the number of items present. 
    /// A palette index of u16::MAX indicates an unused slot.
    /// "threshold" is the max number of items before the map is grown.
    /// "random" is used to prevent DoS attacks.
    /// Linear Probing is used to search the map. Find more info here:
    /// https://en.wikipedia.org/wiki/Linear_probing
    cache: NonNull<(u16, u16)>,
    cache_size: u16,
    cache_bits: u16,
    threshold: u16,
    random: u16,

    /// Parameters that aid in index extraction/assignment.
    /// BPI is short for "bits-per-index". 
    /// IPU is short for "indices-per-usize" (or per word).
    /// 
    /// The BPI is based on the length of the Palette. 
    /// For example, if the length of the palette is 256, 
    /// the bpi is 8, because 8 bits are needed to store 
    /// an index in the palette. 
    ipu_div: u8,
    bpi_mul: u8,
    ipu_mod: usize,
    bpi_mask: usize,

    /// Allocator used for the pointers. Right now
    /// this is the Global Allocator, but in the future
    /// I want to make this a custom region allocator.
    alloc: A,
}

impl<A: Allocator> PaletteArray<A> {
    /// Allocate a PaletteArray with a capacity of 1 (air only). 
    /// 
    /// We're using statics here instead of `Option<NonNull<T>>`, which allows our
    /// .get()s to be branchless - this DID result in a significant performance improvement.
    /// 
    /// As long as we don't assign to the pointers before initializing, we're fine. 
    /// We break this rule in the `words` pointer, but we only ever assign 0 so its a non-issue.
    #[allow(static_mut_refs)]
    pub fn empty(alloc: A) -> Self {
        let random = init_random_state();
        unsafe {
            Self {
                palette: NonNull::new_unchecked(&BPI_ZERO_PALETTE as *const _ as *mut _),
                palette_len: 1,
                palette_cap: 1, 
                words: NonNull::new_unchecked(&BPI_ZERO_WORD as *const _ as *mut _),
                cache: NonNull::new_unchecked(&EMPTY_CACHES[(random & 0xF) as usize] as *const _ as *mut _),
                cache_size: 0,
                cache_bits: 0xF,
                threshold: 11, 
                random,
                bpi_mul: Bpi::BPI0.bpi_mul,
                ipu_div: Bpi::BPI0.ipu_div,
                ipu_mod: Bpi::BPI0.ipu_mod,
                bpi_mask: Bpi::BPI0.bpi_mask,
                alloc,
            }
        }
    }

    pub fn with_palette_capacity(cap: usize, alloc: A) -> Self {
        debug_assert!(cap < 65536);
        let bpi = Bpi::from_palette_cap(cap);
        if bpi.bpi_mask == 0 {
            Self::empty(alloc)
        } else {
            let mut palette_cap = cap.next_power_of_two().max(16);
            if cap > 16 && cap < 128 { palette_cap = 128 };
            let palette = unsafe {
                let layout = Layout::array::<u16>(palette_cap).unwrap();
                let ptr = alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<u16>();
                ptr.write(0); // first element of the palette is always 0
                ptr
            };
            
            let words = {
                let layout = Layout::array::<usize>(words_len(bpi.ipu_div)).unwrap();
                alloc.allocate_zeroed(layout).unwrap().as_non_null_ptr().cast::<usize>()
            };

            let random = init_random_state();
            #[allow(static_mut_refs)]
            let cache = unsafe {
                NonNull::new_unchecked(&EMPTY_CACHES[(random & 0xF) as usize] as *const _ as *mut _)
            };

            Self {
                palette,
                palette_len: 1,
                palette_cap: 1,
                words,
                cache,
                cache_size: 0,
                cache_bits: 0xF,
                threshold: 11,
                random,
                ipu_div: bpi.ipu_div,
                bpi_mul: bpi.bpi_mul,
                ipu_mod: bpi.ipu_mod,
                bpi_mask: bpi.bpi_mask,
                alloc
            }
        }
    }

    /// Extract the voxel state at the index.
    #[inline(always)]
    pub unsafe fn get(&self, idx: usize) -> u16 {
        debug_assert!(idx < 32768, "Index out of bounds: '{idx}'");
        unsafe {
            let word = *self.words.add(idx >> self.ipu_div).as_ptr();
            let offs = (idx & self.ipu_mod) << self.bpi_mul;
            let pidx = (word >> offs) & self.bpi_mask;
            *self.palette.add(pidx).as_ptr()
        }
    }

    /// Assign to the voxel state at this index.
    #[inline(always)]
    pub unsafe fn set(&mut self, idx: usize, val: u16) {
        debug_assert!(idx < 32768, "Index out of bounds: '{idx}'");
        unsafe {
            let pidx = self.search(val);
            let word = self.words.add(idx >> self.ipu_div).as_mut();
            let offs = (idx & self.ipu_mod) << self.bpi_mul;
            let clear = *word & !(self.bpi_mask << offs);
            *word = clear | (pidx << offs);
        }
    }

    /// Assign to the voxel state at this index, returning the previous value.
    #[inline(always)]
    pub unsafe fn replace(&mut self, idx: usize, val: u16) -> u16 {
        debug_assert!(idx < 32768, "Index out of bounds: '{idx}'");
        unsafe {
            let pidx = self.search(val);
            let word = self.words.add(idx >> self.ipu_div).as_mut();
            let offs = (idx & self.ipu_mod) << self.bpi_mul;
            let old = (*word >> offs) & self.bpi_mask;
            *word ^= (old ^ pidx) << offs;
            *self.palette.add(old).as_ptr()
        }
    }

    #[inline(always)]
    pub unsafe fn get_span(&self, start: usize, span: &mut [Voxel]) {
        for i in 0..span.len() {
            span[i] = Voxel(unsafe { self.get(start + i) })
        }
    }

    #[inline(always)]
    pub unsafe fn set_span(&mut self, start: usize, span: &[Voxel]) {
        for i in 0..span.len() {
            unsafe { self.set(start + i, span[i].0) }
        }
    }

    #[inline(always)]
    fn search(&mut self, key: u16) -> usize {
        unsafe {
            let mut index = ((key ^ self.random) & self.cache_bits) as usize;
            loop {
                let entry = *self.cache.add(index).as_ptr();

                // key found, return index.
                if entry.0 == key {
                    return entry.1 as usize;
                }

                // An index of 65535 means the spot is unused.
                if entry.1 == u16::MAX {
                    // resolve key to an index in the palette and assign.
                    let pidx = self.find_or_insert_in_palette(key);
                    *self.cache.add(index).as_mut() = (key, pidx as u16);
                    self.cache_size += 1;

                    // grow the cache if the load factor is too high
                    if self.cache_size > self.threshold {
                        self.grow_cache();
                    }

                    return pidx;
                } 

                // advance to next spot.
                index = (index + 1) & self.cache_bits as usize;
            }
        }  
    }

    /// Double the cache size.
    #[inline(never)]
    fn grow_cache(&mut self) {
        // compute new/old size
        let old_size = (self.cache_bits + 1) as usize;
        let new_size = old_size << 1;
        let new_bits = new_size - 1;

        // allocate new pointer
        let old_cache = self.cache;
        let new_cache = unsafe {
            let new_layout = Layout::array::<(u16, u16)>(new_size).unwrap();
            let ptr = self.alloc.allocate(new_layout).unwrap().as_non_null_ptr().cast::<_>();
            // initialize items to (0, MAX)
            for i in 0..new_size {
                ptr.add(i).write((0, u16::MAX));
            }
            ptr
        };

        // insert old values into new ptr
        for i in 0..old_size {
            unsafe {
                let item = *old_cache.add(i).as_ptr();
                if item.1 != u16::MAX {
                    let mut index = (item.0 ^ self.random) as usize & new_bits;
                    while new_cache.add(index).as_ref().1 != u16::MAX {
                        index = (index + 1) & new_bits;
                    }
                    *new_cache.add(index).as_mut() = (item.0, item.1);
                }
            }
        }

        // deallocate old ptr
        unsafe { 
            let old_layout = Layout::array::<(u16, u16)>(old_size).unwrap();
            self.alloc.deallocate(old_cache.cast::<u8>(), old_layout);
        }

        // assign new pointer and params
        self.cache = new_cache;
        self.cache_bits = new_bits as u16;
        self.threshold = (new_size - (new_size >> 2)) as u16; // load factor of 75%
    }

    #[inline(never)]
    fn find_or_insert_in_palette(&mut self, key: u16) -> usize {
        unsafe {
            // initialize cache if empty
            if self.cache_size == 0 {
                let layout = Layout::array::<(u16, u16)>(16).unwrap();
                self.cache = self.alloc.allocate(layout)
                    .unwrap().as_non_null_ptr().cast::<(u16, u16)>();
                for i in 0..16 {
                    self.cache.add(i).write((0, u16::MAX));
                }
            }

            let mut i = 0;

            // SIMD search is faster than linear search when there are more than 128 keys.
            // this is especially true on AVX512, but holds its own on SSE and AVX2 as well.
            if self.palette_len >= 128 {
                const L: usize = 8;
                let tar: Simd<u16, L> = Simd::splat(key);
                let palette = std::slice::from_raw_parts(self.palette.as_ptr(), self.palette_len as usize);
                let end = self.palette_len as usize & !(L - 1);
                while i < end {
                    if let Some(j) = Simd::from_slice(&palette[i..]).simd_eq(tar).first_set() {
                        return i + j;
                    } else {
                        i += L;
                    }
                }
            }

            // Either searches the entire palette with linear search, or just 
            // the remainder of simd search (if any). 
            for i in i..self.palette_len as usize {
                if *self.palette.add(i).as_ref() == key {
                    return i;
                }
            }

            // search failed; grow palette / index buffer if out of space.
            if self.palette_len >= self.palette_cap {
                self.grow_palette();
            }

            // Push palette key to end.
            let pidx = self.palette_len as usize;
            self.palette.add(pidx).write(key);
            self.palette_len += 1;
            pidx
        }
    }

    /// Doubles the capacity of the palette.
    /// If the BPI has increased, double the capacity of words.
    fn grow_palette(&mut self) {
        if self.palette_cap == 1 {
            // Initialize palette with cap 16
            self.palette_cap = 16;
            self.palette = unsafe {
                let layout = Layout::array::<u16>(16).unwrap();
                let ptr = self.alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<u16>();
                ptr.write(0);
                ptr
            };

            // update bpi to 4
            let new_bpi = Bpi::BPI4;
            self.bpi_mul = new_bpi.bpi_mul;
            self.ipu_div = new_bpi.ipu_div;
            self.ipu_mod = new_bpi.ipu_mod;
            self.bpi_mask = new_bpi.bpi_mask;

            // initialize index buffer
            let layout = Layout::array::<usize>(words_len(new_bpi.ipu_div)).unwrap();
            self.words = self.alloc.allocate_zeroed(layout).unwrap().as_non_null_ptr().cast::<usize>();

            // everything we need to do is done, return. 
            return;
        } 

        // Palette already initialized; reallocate to double the current cap.
        let old_cap = self.palette_cap as usize;
        let new_cap = old_cap << 1;
        let old_layout = Layout::array::<u16>(old_cap).unwrap();
        let new_layout = Layout::array::<u16>(new_cap).unwrap();
        self.palette_cap = new_cap as u16;
        self.palette = unsafe {
            self.alloc.grow(self.palette.cast::<u8>(), old_layout, new_layout)
                .unwrap().as_non_null_ptr().cast::<u16>()
        };

        // grow the index buffer is the new capacity is too large.
        if self.palette_cap > max_palette_cap(self.bpi_mask) {
            let old_bpi = self.bpi();
            let new_bpi = old_bpi.next();

            // double capacity of words pointer.
            // BPI must not be 0 at this point.
            self.words = unsafe {
                let old_layout = Layout::array::<usize>(words_len(old_bpi.ipu_div)).unwrap();
                let new_layout = Layout::array::<usize>(words_len(new_bpi.ipu_div)).unwrap();
                self.alloc.grow(self.words.cast::<u8>(), old_layout, new_layout)
                    .unwrap().as_non_null_ptr().cast::<usize>()
            };

            if old_bpi.bpi_mask == 0xF {
                // expand from BPI=4 to BPI=8
                for i in (0..words_len(Bpi::BPI4.ipu_div)).rev() {
                    let k = i << 1;
                    unsafe {
                        let (lo, hi) = expand_bpi::<4>(*self.words.add(i).as_ptr());
                        *self.words.add(k).as_mut() = lo;
                        *self.words.add(k+1).as_mut() = hi;                            
                    }
                }
            } else if old_bpi.bpi_mask == 0xFF {
                // expand from BPI=8 to BPI=16
                for i in (0..words_len(Bpi::BPI8.ipu_div)).rev() {
                    let k = i << 1;
                    unsafe {
                        let (lo, hi) = expand_bpi::<8>(*self.words.add(i).as_ptr());
                        *self.words.add(k).as_mut() = lo;
                        *self.words.add(k+1).as_mut() = hi;                            
                    }
                }
            } else {
                unreachable!("Index Buffer Overflow");
            }

            // update bpi
            self.bpi_mul = new_bpi.bpi_mul;
            self.ipu_div = new_bpi.ipu_div;
            self.ipu_mod = new_bpi.ipu_mod;
            self.bpi_mask = new_bpi.bpi_mask;
        }
    }

    fn bpi(&self) -> Bpi {
        Bpi {
            ipu_div: self.ipu_div,
            bpi_mul: self.bpi_mul,
            ipu_mod: self.ipu_mod,
            bpi_mask: self.bpi_mask,
        }
    }
}

impl<A: Allocator> Drop for PaletteArray<A> {
    fn drop(&mut self) {
        unsafe {
            if self.palette_cap != 1 {
                // deallocate palette
                let layout = Layout::array::<u16>(self.palette_cap as usize).unwrap();
                self.alloc.deallocate(self.palette.cast::<u8>(), layout);
                // deallocate words
                let layout = Layout::array::<usize>(words_len(self.ipu_div)).unwrap();
                self.alloc.deallocate(self.words.cast::<u8>(), layout);
            }

            if self.cache_size != 0 {
                // deallocate cache
                let layout = Layout::array::<(u16, u16)>((self.cache_bits + 1) as usize).unwrap();
                self.alloc.deallocate(self.cache.cast::<u8>(), layout);
            }
        }
    }
}

/// Expands the bpi from OLD to OLD*2
/// Only intended to be used with OLD=4 and OLD=8. Anything else is invalid.
/// Returns the (lower, upper) value.
#[inline(always)]
fn expand_bpi<const OLD: usize>(word: usize) -> (usize, usize) {
    const HALF: usize = usize::BITS as usize / 2;

    // Extract the lower/upper 32 bits
    let mut lower = word & const { (1 << HALF) - 1 };
    let mut upper = word >> HALF;

    // lower/upper output variables
    let (mut r1, mut r2) = (0, 0);

    // sliding window for selecting only relevant bits from lower/upper
    let mut mask = const { (1 << OLD) - 1 };

    // execute expansion from old to new into r1/r2
    for _ in 0..const { usize::BITS as usize / (OLD * 2) } {
        r1 |= lower & mask;
        r2 |= upper & mask;
        lower <<= OLD;
        upper <<= OLD;
        mask <<= const { OLD * 2 };
    }

    (r1, r2)
}

#[derive(Debug, Copy, Clone)]
struct Bpi {
    /// SHR Factor for dividing by the indices-per-usize
    ipu_div: u8,

    /// SHL Factor for multiplying by the bits-per-index
    bpi_mul: u8,

    /// AND Factor for modulo by the indices-per-usize
    ipu_mod: usize,

    /// Mask of the first N bits. 
    /// If BPI=4, then this is equal to 0xF.
    bpi_mask: usize,
}

impl Bpi {
    fn next(&self) -> Self {
        match self.bpi_mask.count_ones() {
            0 => Self::BPI4,
            4 => Self::BPI8,
            8 => Self::BPI16,
            _ => unreachable!("Palette Overflow.")
        }
    }

    const fn from_palette_cap(cap: usize) -> Self {
        match cap {
            ..=1 => Self::BPI0,
            ..=16 => Self::BPI4,
            ..=256 => Self::BPI8,
            _ => Self::BPI16,
        }
    }

    const fn new<const BPI: usize>() -> Self {
        let ipu = usize::BITS as usize / BPI;
        let mut offsets = [0; 16];
        let mut i = 0;
        while i < ipu {
            // Calculate offset from the right for all indices
            offsets[i] = (BPI * i) as u8;
            i += 1;
        }

        Self {
            ipu_div: ipu.trailing_zeros() as u8,
            bpi_mul: BPI.trailing_zeros() as u8,
            ipu_mod: ipu - 1,
            bpi_mask: (1 << BPI) - 1,
        }
    }

    const BPI4: Self = Self::new::<4>();
    const BPI8: Self = Self::new::<8>();
    const BPI16: Self = Self::new::<16>();
    const BPI0: Self = Self {
        ipu_div: 15,
        bpi_mul: 0,
        ipu_mod: 0,
        bpi_mask: 0,
    };
}

const fn max_palette_cap(bpi_mask: usize) -> u16 {
    bpi_mask as u16 + 1
}

const fn words_len(ipu_div: u8) -> usize {
    32768 / (1 << ipu_div) 
}

std::thread_local! {
    static STATE: OnceCell<RefCell<u32>> = OnceCell::new();
}

#[cfg(not(target_family = "wasm"))]
fn init_random_state() -> u16 {
    use std::time::{SystemTime, UNIX_EPOCH};
    STATE.with(|cell| {
        cell.get_or_init(move || {
            let state = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_else(|_| Duration::from_secs(3753857837));
            RefCell::new((state.as_nanos() & 0xFFFFFFFF) as u32)
        }).replace_with(|state| {
            let r = state.wrapping_mul(3094417873).wrapping_add(177818209);
            (r >> 16) ^ r
        }) & 0xFFFF
    }) as u16
}

#[cfg(target_family = "wasm")]
fn init_random_state() -> u16 {
    STATE.with(|cell| {
        cell.get_or_init(move || {
            RefCell::new(2834261329)
        }).replace_with(|state| {
            let r = state.wrapping_mul(3094417873).wrapping_add(177818209);
            (r >> 16) ^ r
        }) & 0xFFFF
    }) as u16
}

static mut EMPTY_CACHES: [[(u16, u16); 16]; 16] = {
    let mut result = [[(u16::MAX, u16::MAX); 16]; 16];
    let mut i = 0;
    while i < 16 {
        result[i][i] = (0, 0);
        i += 1;
    }
    result
};

#[cfg(test)]
mod tests {
    use super::PaletteArray;
    use crate::tests::TestRng;

    #[test]
    fn palette_random() {
        let mut arr = PaletteArray::empty(std::alloc::Global);
        let mut rng = TestRng::new(0x3738787387391);
        let mut nums = Vec::new();

        for i in 0..32768 {
            unsafe { arr.set(i, (i & 7) as u16) }
            // assert_eq!(unsafe { arr.replace(i, (i & 7) as u16) }, 0);
        }

        for i in 0..32768 {
            let r = (rng.next() & 511) as u16;
            nums.push(r);
            assert_eq!(unsafe { arr.replace(i, r) }, (i & 7) as u16);
        }

        for i in 0..32768 {
            assert_eq!(unsafe { arr.get(i) }, nums[i]);
        }
    }
}

use std::alloc::{Allocator, Global, Layout};
use std::cmp::Ordering;
use std::simd::cmp::SimdPartialEq;
use std::simd::prelude::*;
use std::{hint, mem};
use std::ptr::NonNull;

use crate::consts::{SIMD_LANES, SUBCHUNK_LENGTH};

static mut BPI_ZERO_WORD: usize = 0;
static mut BPI_ZERO_PALETTE: u16 = 0;

pub struct PaletteArray<A: Allocator = Global> {
    words: NonNull<usize>,
    palette: NonNull<u16>,
    cache: NonNull<Cache>,
    bpi: &'static Bpi,
    palette_len: u32,
    palette_cap: u32,
    alloc: A,
}

impl<A: Allocator> PaletteArray<A> {
    /// Initialize a new PaletteArray where all voxels are 0. 
    /// We're using statics here to set the pointers to because we
    /// don't want to have to check if the palette is empty or not every access.
    /// In order to mutate the voxels, we would have to push to cache, which would mean
    /// growing the buffer. THATs where we allocate a pointer that can actually 
    /// be mutated. 
    #[allow(static_mut_refs)]
    pub const fn empty(alloc: A) -> Self {
        unsafe {
            Self {
                palette: NonNull::new_unchecked(&BPI_ZERO_PALETTE as *const u16 as *mut u16),
                palette_len: 1,
                palette_cap: 1,
                words: NonNull::new_unchecked(&BPI_ZERO_WORD as *const usize as *mut usize),
                cache: NonNull::new_unchecked(&Cache::EMPTY as *const _ as *mut _),
                bpi: &Bpi::BPI0,
                alloc,
            }
        }
    }

    pub fn with_palette_capacity(cap: usize, alloc: A) -> Self {
        debug_assert!(cap < 65536);
        let bpi = Bpi::from_palette_cap(cap);
        if bpi.words_len == 1 {
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
            
            let words = unsafe {
                let layout = Layout::array::<usize>(bpi.words_len as usize).unwrap();
                let ptr = alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<usize>();
                ptr.write_bytes(0, bpi.words_len as usize);
                ptr
            };

            #[allow(static_mut_refs)]
            let cache = unsafe {
                NonNull::new_unchecked(&Cache::EMPTY as *const _ as *mut _)
            };

            Self {
                palette,
                palette_len: 1,
                palette_cap: 1,
                words,
                cache,
                bpi,
                alloc
            }
        }
    }

    fn cache_is_init(&self) -> bool {
        unsafe { self.cache.as_ref().is_init } 
    }

    #[inline]
    pub fn get(&self, idx: usize) -> u16 {
        debug_assert!(idx < 32768);
        unsafe {
            let bpi = *self.bpi;
            let offset = self.bpi.offsets.get_unchecked(idx & bpi.ipu_mod);
            let word = *self.words.as_ptr().add(idx >> bpi.ipu_div);
            let pal_idx = (word >> offset) & bpi.bpi_mask;
            *self.palette.as_ptr().add(pal_idx)
        }
    }

    #[inline]
    pub fn set(&mut self, idx: usize, val: u16) {
        debug_assert!(idx < 32768);
        unsafe {
            let pidx = self.search_cache(val);
            let bpi = *self.bpi;
            let offs = *self.bpi.offsets.get_unchecked(idx & bpi.ipu_mod);
            let word = self.words.as_ptr().add(idx >> bpi.ipu_div);
            *word = (*word & !(bpi.bpi_mask << offs)) | (pidx << offs);
        }
    }

    #[inline]
    pub fn replace(&mut self, idx: usize, val: u16) -> u16 {
        debug_assert!(idx < 32768);
        unsafe {
            let pidx = self.search_cache(val);
            let bpi = *self.bpi;
            let offs = *self.bpi.offsets.get_unchecked(idx & bpi.ipu_mod);
            let word = self.words.as_ptr().add(idx >> bpi.ipu_div);
            let old = (*word >> offs) & bpi.bpi_mask;
            *word = (*word & !(bpi.bpi_mask << offs)) | (pidx << offs);
            *self.palette.add(old).as_ptr()
        }
    }

    /// Modify a voxel and return the old value.
    #[inline]
    pub fn update<F>(&mut self, idx: usize, f: F) -> u16 
    where
        F: FnOnce(u16) -> u16,
    {
        unsafe {
            let bpi = *self.bpi;
            let offs = *self.bpi.offsets.get_unchecked(idx & bpi.ipu_mod);
            let word = self.words.as_ptr().add(idx >> bpi.ipu_div);
            let old_index = (*word >> offs) & bpi.bpi_mask;
            let old_voxel = *self.palette.add(old_index).as_ptr();
            let new = (f)(old_voxel);
            if new != old_voxel {
                let pidx = self.search_cache(new);
                *word = (*word & !(bpi.bpi_mask << offs)) | (pidx << offs);
            }
            old_voxel
        }
    }

    #[inline(always)]
    fn search_cache(&mut self, val: u16) -> usize {
        unsafe { self.cache.as_mut() }.search(val)
            .unwrap_or_else(|i| self.find_or_insert(val, i))
    }

    #[inline(never)]
    fn find_or_insert(&mut self, key: u16, cache_idx: usize) -> usize {
        unsafe {
            if !self.cache_is_init() {
                let layout = Layout::new::<Cache>();
                let mut ptr = self.alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<Cache>();
                ptr.write(Cache::EMPTY);
                ptr.as_mut().is_init = true;
                self.cache = ptr;
            }

            let mut i = 0;

            // SIMD search is faster than linear search when there are more than 128 keys.
            // this is especially true on AVX512. 
            if self.palette_len >= 128 {
                const L: usize = SIMD_LANES / 2;
                let tar: Simd<u16, L> = Simd::splat(key);
                let palette = std::slice::from_raw_parts(self.palette.as_ptr(), self.palette_len as usize);
                let end = self.palette_len as usize & !(L - 1);
                while i < end {
                    if let Some(j) = Simd::from_slice(&palette[i..]).simd_eq(tar).first_set() {
                        let k = i + j;
                        self.cache.as_mut().insert(key, k, cache_idx);
                        return k;
                    }

                    i += L;
                }
            }

            // Either searches the entire palette with linear search, or just 
            // the remainder of simd search. 
            for i in i..self.palette_len as usize {
                if *self.palette.add(i).as_ref() == key {
                    self.cache.as_mut().insert(key, i, cache_idx);
                    return i;
                }
            }

            // allocate more space if needed
            if self.palette_len >= self.palette_cap {
                self.grow_palette();
                if self.palette_cap > self.bpi.palette_cap {
                    self.grow_words();
                }
            }

            // assign key to palette and insert into cache
            let pidx = self.palette_len as usize;
            self.palette.add(pidx).write(key);
            self.palette_len += 1;
            self.cache.as_mut().insert(key, pidx, cache_idx);
            pidx
        }
    }

    /// Double the capacity of the palette.
    fn grow_palette(&mut self) {
        match self.palette_cap {
            // initialize
            1 => {
                self.palette_cap = 16;
                self.palette = unsafe {
                    let layout = Layout::array::<u16>(16).unwrap();
                    let ptr = self.alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<u16>();
                    ptr.write(0);
                    ptr
                };
            },
            // 16 grows directly to 128
            16 => {
                self.palette_cap = 128;
                self.palette = unsafe {
                    let old_layout = Layout::array::<u16>(16).unwrap();
                    let new_layout = Layout::array::<u16>(128).unwrap();
                    self.alloc.grow(self.palette.cast::<u8>(), old_layout, new_layout)
                        .unwrap().as_non_null_ptr().cast::<u16>()
                };
            },
            _ => {
                let old_cap = self.palette_cap;
                self.palette_cap <<= 1;
                self.palette = unsafe {
                    let old_layout = Layout::array::<u16>(old_cap as usize).unwrap();
                    let new_layout = Layout::array::<u16>(self.palette_cap as usize).unwrap();
                    self.alloc.grow(self.palette.cast::<u8>(), old_layout, new_layout)
                        .unwrap().as_non_null_ptr().cast::<u16>()
                }
            }
        }
    }

    /// Double the capacity of words, or initialize.
    fn grow_words(&mut self) {
        let old_bpi = self.bpi;
        let new_bpi = old_bpi.next();
        match old_bpi.bpi_mask.count_ones() {
            0 => {
                // allocate new words.
                // We don't de-allocate the old words because it points to a static.
                self.words = unsafe {
                    let layout = Layout::array::<usize>(new_bpi.words_len as usize).unwrap();
                    let ptr = self.alloc.allocate_zeroed(layout).unwrap().as_non_null_ptr().cast::<usize>();
                    ptr.write_bytes(0, new_bpi.words_len as usize); // < wtf? spent like 2 hours on this
                    ptr
                };
            },
            4 => self.expand_word_bits::<4, 8>(&old_bpi, &new_bpi),
            8 => self.expand_word_bits::<8, 16>(&old_bpi, &new_bpi),
            _ => panic!("[PA339] Index Buffer Overflow."),
        }
        self.bpi = new_bpi;
    }

    fn expand_word_bits<
        const OLD_BPI: usize,
        const NEW_BPI: usize,
    >(&mut self, old_bpi: &Bpi, new_bpi: &Bpi) {
        let bpi_mask = (1 << OLD_BPI) - 1;
        const HALF: usize = usize::BITS as usize / 2;
        // reallocate words pointer
        self.words = unsafe {
            let old_layout = Layout::array::<usize>(old_bpi.words_len as usize).unwrap();
            let new_layout = Layout::array::<usize>(new_bpi.words_len as usize).unwrap();
            self.alloc.grow(self.words.cast::<u8>(), old_layout, new_layout).unwrap().as_non_null_ptr().cast::<usize>()
        };
        // perform bit expansion
        for i in (0..old_bpi.words_len as usize).rev() {
            let word = unsafe { *self.words.add(i).as_ptr() };
            let mut lower = word & const { (1 << HALF) - 1 };
            let mut upper = word >> HALF;
            let (mut r1, mut r2) = (0, 0);
            let mut mask = bpi_mask;
            for _ in 0..const { usize::BITS as usize / NEW_BPI } {
                r1 |= lower & mask;
                r2 |= upper & mask;
                lower <<= OLD_BPI;
                upper <<= OLD_BPI;
                mask <<= NEW_BPI;
            }
            let k = i << 1;
            unsafe {
                #[cfg(test)]
                assert!(k+1 < new_bpi.words_len as usize);
                *self.words.add(k).as_mut() = r1;
                *self.words.add(k+1).as_mut() = r2;
            }
        }
    }

    /// Enumerate the indices of the array.
    /// This function is roughly twice as fast as iterating with `PaletteArray::get`.
    pub fn for_each<F>(&self, mut f: F) 
    where
        F: FnMut(usize, u16)
    {
        #[inline]
        fn iter_with_bpi<const BPI: u32, A: Allocator>(pal: &PaletteArray<A>, mut f: impl FnMut(usize, u16)) {
            unsafe {
                let mut i = 0;
                for j in 0..pal.bpi.words_len as usize {
                    let mut w = *pal.words.add(j).as_ref();
                    for _ in 0..const { usize::BITS / BPI } {
                        (f)(i, *pal.palette.add(w & const { (1 << BPI) - 1 }).as_ref());
                        w >>= BPI; i += 1;
                    }
                }
            }
        }

        match self.bpi.bpi_mask.count_ones() {
            0 => {
                for i in 0..SUBCHUNK_LENGTH {
                    (f)(i, 0)
                }
            },
            4 => iter_with_bpi::<4, A>(self, f),
            8 => iter_with_bpi::<8, A>(self, f),
            16 => iter_with_bpi::<16, A>(self, f),
            _ => unreachable!()
        }
    }

    pub fn from_fn<F>(alloc: A, mut f: F) -> Self 
    where
        F: FnMut(usize) -> u16
    {
        let mut i = 0;
        let mut val = (f)(i);
        let mut ret = Self::empty(alloc);

        while val == 0 {
            i += 1;
            if i >= SUBCHUNK_LENGTH { return ret };
            val = (f)(i);
        }

        loop {
            ret.grow_words();
            let bpi = *ret.bpi;
            while let Some(j) = ret.find_or_insert_non_grow(val) {
                unsafe {
                    let w = ret.words.add(i >> bpi.ipu_div).as_mut();
                    *w |= j << bpi.offsets[i & bpi.ipu_mod];
                    i += 1;
                    if i >= SUBCHUNK_LENGTH { return ret; }
                    val = (f)(i);
                }
            }
        }
    }   

    /// Linear search the palette directly without the cache, returning
    /// None if a palette fault occurrs. Will still grow the palette if
    /// possible.
    fn find_or_insert_non_grow(&mut self, key: u16) -> Option<usize> {
        unsafe {
            let slice = std::slice::from_raw_parts(self.palette.as_ptr(), self.palette_len as usize);
            for (i, k) in slice.iter().enumerate() {
                if *k == key {
                    return Some(i)
                }
            }

            if self.palette_len == self.palette_cap {
                self.grow_palette();
                if self.palette_cap > self.bpi.palette_cap {
                    return None
                }
            }

            let i = self.palette_len as usize;
            self.palette.add(i).write(key);
            Some(i)
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
                let layout = Layout::array::<usize>(self.bpi.words_len as usize).unwrap();
                self.alloc.deallocate(self.words.cast::<u8>(), layout);
                // deallocate cache if init
                if self.cache_is_init() {
                    let layout = Layout::new::<Cache>();
                    self.alloc.deallocate(self.cache.cast::<u8>(), layout);
                }
            }
        }
    }
}

#[derive(Debug, Copy, Clone)]
struct Bpi {
    ipu_div: u32,
    ipu_mod: usize,
    bpi_mask: usize,
    offsets: [u8; 16],    
    words_len: u32,
    palette_cap: u32,
}

impl Bpi {
    fn next(&self) -> &'static Self {
        match self.bpi_mask.count_ones() {
            0 => &Self::BPI4,
            4 => &Self::BPI8,
            8 => &Self::BPI16,
            _ => unreachable!("[PA221] Overflow.")
        }
    }

    const fn from_palette_cap(cap: usize) -> &'static Self {
        match cap {
            ..=1 => &Self::BPI0,
            ..=16 => &Self::BPI4,
            ..=256 => &Self::BPI8,
            _ => &Self::BPI16,
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
            ipu_div: ipu.trailing_zeros(),
            ipu_mod: ipu - 1,
            words_len: (SUBCHUNK_LENGTH / ipu) as u32,
            bpi_mask: (1 << BPI) - 1,
            offsets,
            palette_cap: u32::pow(2, BPI as u32),
        }
    }

    const BPI4: Self = Self::new::<4>();
    const BPI8: Self = Self::new::<8>();
    const BPI16: Self = Self::new::<16>();
    const BPI0: Self = Self {
        ipu_div: 15,
        ipu_mod: 0,
        bpi_mask: 0,
        words_len: 1,
        offsets: [0; 16],
        palette_cap: 1,
    };
}

struct Cache {
    len: usize,
    buckets: u16x16,
    items: [(u16, u16); 128],
    is_init: bool,
}

impl Cache {
    const EMPTY: Self = {
        let mut result = [(u16::MAX, u16::MAX); 128];
        result[0] = (0, 0);
        Self {
            buckets: u16x16::splat(u16::MAX),
            items: result,
            len: 1,
            is_init: false,
        }
    };

    fn insert(&mut self, key: u16, palette_idx: usize, cache_idx: usize) {
        if cache_idx >= 127 {
            self.items[127] = (key, palette_idx as u16);
            self.buckets[15] = key;
            self.len = 128;
        } else {
            self.len = (self.len+1).min(128);

            // shift elements over
            for i in ((cache_idx+1)..self.len).rev() {
                self.items[i] = self.items[i-1];
            }

            // assign new item
            self.items[cache_idx] = (key, palette_idx as u16);

            // update buckets in shifted range
            for i in (cache_idx >> 3)..(self.len >> 3) {
                self.buckets[i] = self.items[((i+1) << 3) - 1].0;
            }
        }
    }

    /// Binary search the cache.
    /// If `Ok` is returned, it is an index in the palette.
    /// If `Err` is returned, it is the index in the cache the key could be inserted.
    #[inline(always)]
    fn search(&self, key: u16) -> Result<usize, usize> {
        // get the index of the containing bucket
        let mask = u16x16::splat(key).simd_le(self.buckets).to_bitmask();
        if mask == 0 { return Err(128); }
        let mut idx = (mask.trailing_zeros() as usize) << 3;
        let mut mid = idx + 4;
        // binary search the bucket (width=8)
        idx = hint::select_unpredictable(key >= self.items[mid].0, mid, idx);
        mid = idx + 2;
        idx = hint::select_unpredictable(key >= self.items[mid].0, mid, idx);
        mid = idx + 1;
        idx = hint::select_unpredictable(key >= self.items[mid].0, mid, idx);
        // check whether the result is eq
        let item = self.items[idx];
        let cmp = item.0.cmp(&key);
        if cmp == Ordering::Equal {
            Ok(item.1 as usize)
        } else {
            Err(idx + (cmp == Ordering::Less) as usize)
        }
    }
}

#[cfg(test)]
mod tests {

    use crate::{region::PaletteArray, tests::TestRng};

    use super::Cache;

    #[test]
    fn cache_search() {
        let mut cache = Cache::EMPTY;
        cache.is_init = true;

        cache.insert(3, 9, cache.search(3).unwrap_err());
        cache.insert(2, 11, cache.search(2).unwrap_err());
        cache.insert(1, 15, cache.search(1).unwrap_err());

        assert_eq!(cache.search(0), Ok(0));
        assert_eq!(cache.search(1), Ok(15));
        assert_eq!(cache.search(2), Ok(11));
        assert_eq!(cache.search(3), Ok(9));
        assert_eq!(cache.search(4), Err(4));
    }

    #[test]
    fn palette_random() {
        let mut arr = PaletteArray::empty(std::alloc::Global);
        let mut rng = TestRng::new(0x3738787387391);

        let mut nums = Vec::new();

        for i in 0..32768 {
            let r = (rng.next() & 511) as u16;
            nums.push(r);
            arr.set(i, r);
        }

        for i in 0..32768 {
            assert_eq!(arr.get(i), nums[i]);
        }
    }
}
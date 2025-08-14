

use std::{alloc::{Allocator, Global, Layout}, ptr::NonNull, sync::Arc};

#[derive(Copy, Clone, Eq, PartialEq, Debug)]
pub struct Light {
    /// 4 bits ambient intensity, 4 bits torch intensity.
    pub intensity: u8,

    /// 4 bits HSL hue, 4 bits HSL lightness
    pub hsl_color: u8,
}

impl Light {
    /// Ambient channel = 16
    pub const fn full() -> Self {
        Self {
            intensity: 0x0F,
            hsl_color: 0,
        }
    }

    /// No light
    pub const fn none() -> Self {
        Self {
            intensity: 0,
            hsl_color: 0,
        }
    }
}

static LIGHTMAP_UNIFORM_FULL: [Light; 32768] = [const { Light::full() }; 32768];
static LIGHTMAP_UNIFORM_NONE: [Light; 32768] = [const { Light::none() }; 32768];

pub struct LightMap<A: Allocator = Global> {
    ptr: NonNull<Light>,
    is_uniform: bool,
    alloc: A,
}

impl<A: Allocator> LightMap<A> {
    pub fn uniform_full(alloc: A) -> Self {
        Self {
            ptr: unsafe { NonNull::new_unchecked(&LIGHTMAP_UNIFORM_FULL as *const _ as *mut _) },
            is_uniform: true,
            alloc
        }
    }

    pub fn uniform_none(alloc: A) -> Self {
        Self {
            ptr: unsafe { NonNull::new_unchecked(&LIGHTMAP_UNIFORM_FULL as *const _ as *mut _) },
            is_uniform: true,
            alloc
        }
    }

    pub fn set_uniform_full(&mut self) {
        self.ptr = unsafe { NonNull::new_unchecked(&LIGHTMAP_UNIFORM_FULL as *const _ as *mut _) };
        self.is_uniform = true;
    }

    pub fn set_uniform_none(&mut self) {
        self.ptr = unsafe { NonNull::new_unchecked(&LIGHTMAP_UNIFORM_NONE as *const _ as *mut _) };
        self.is_uniform = true;
    }

    pub fn get(&self, idx: usize) -> Option<Light> {
        (idx < 32768).then(|| unsafe { self.get_unchecked(idx) })
    }

    pub unsafe fn get_unchecked(&self, idx: usize) -> Light {
        #[cfg(test)]
        assert!(idx < 32768);
        unsafe { *self.ptr.add(idx).as_ref() }
    }

    pub fn set(&mut self, idx: usize, light: Light) -> Option<Light> {
        (idx < 32768).then(|| unsafe { self.set_unchecked(idx, light) })
    }

    pub unsafe fn set_unchecked(&mut self, idx: usize, light: Light) -> Light {
        #[cfg(test)]
        assert!(idx < 32768);
        unsafe {
            if self.is_uniform {
                if light == *self.ptr.as_ptr() {
                    light
                } else {
                    self.is_uniform = true;
                    let layout = Layout::array::<Light>(32768).unwrap();
                    let ptr = self.alloc.allocate(layout).unwrap().as_non_null_ptr().cast::<Light>();
                    ptr.copy_from(self.ptr, 32768);
                    self.ptr = ptr;
                    std::mem::replace(self.ptr.add(idx).as_mut(), light)                    
                }
            } else {
                std::mem::replace(self.ptr.add(idx).as_mut(), light)
            }
        }
    }
}

pub type Alloc = std::alloc::Global;

pub fn init_allocator() -> Alloc {
    std::alloc::Global
}
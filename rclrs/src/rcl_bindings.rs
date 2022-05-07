#![allow(dead_code)]
#![allow(non_upper_case_globals)]
#![allow(non_camel_case_types)]
#![allow(non_snake_case)]
#![allow(clippy::all)]

include!(concat!(env!("OUT_DIR"), "/rcl_bindings_generated.rs"));

impl Clone for rcutils_allocator_t {
    fn clone(&self) -> Self {
        let allocate = self.allocate.clone();
        let deallocate = self.deallocate.clone();
        let reallocate = self.reallocate.clone();
        let zero_allocate = self.zero_allocate.clone();
        let state = self.state.clone();

        rcutils_allocator_t {
            allocate,
            deallocate,
            reallocate,
            zero_allocate,
            state,
        }
    }
}

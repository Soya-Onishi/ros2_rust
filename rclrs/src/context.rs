use crate::error::{RclReturnCode, ToResult};
use crate::rcl_bindings::*;
use crate::Node;
use alloc::sync::Arc;
use alloc::vec::Vec;
use std::string::String;

#[cfg(not(feature = "std"))]
use cstr_core::{c_char, CString};
#[cfg(not(feature = "std"))]
use spin::Mutex;

#[cfg(feature = "std")]
use cty::c_char;
#[cfg(feature = "std")]
use parking_lot::Mutex;
#[cfg(feature = "std")]
use std::ffi::CString;

impl Drop for rcl_context_t {
    fn drop(&mut self) {
        unsafe {
            rcl_shutdown(self as *mut _);
            rcl_context_fini(self as *mut _);
        }
    }
}

pub struct Context {
    pub(crate) handle: Arc<Mutex<rcl_context_t>>,
}

impl Context {
    /// Creates a new context.
    ///
    /// Usually, this would be called with `std::env::args()`, analogously to `rclcpp::init()`.
    /// See also the official "Passing ROS arguments to nodes via the command-line" tutorial.
    ///
    /// Creating a context can fail in case the args contain invalid ROS arguments.
    ///
    /// # Example
    /// ```
    /// # use rclrs::Context;
    /// assert!(Context::new([]).is_ok());
    /// let invalid_remapping = ["--ros-args", "-r", ":=:*/]"].map(String::from);
    /// assert!(Context::new(invalid_remapping).is_err());
    /// ```
    ///
    /// # Panics
    /// When there is an interior null byte in any of the args.
    pub fn new(args: impl IntoIterator<Item = String>) -> Result<Self, RclReturnCode> {
        let context = Self {
            // SAFETY: Getting a zero-initialized value is always safe
            handle: Arc::new(Mutex::new(unsafe { rcl_get_zero_initialized_context() })),
        };
        let cstring_args: Vec<CString> = args
            .into_iter()
            .map(|arg| CString::new(arg).unwrap())
            .collect();
        // Vector of pointers into cstring_args
        let c_args: Vec<*const c_char> = cstring_args.iter().map(|arg| arg.as_ptr()).collect();
        // Scope for the handle
        {
            let handle = &mut *context.handle.lock();
            unsafe {
                // SAFETY: No preconditions for this function.
                let allocator = rcutils_get_default_allocator();
                // SAFETY: Getting a zero-initialized value is always safe.
                let mut init_options = rcl_get_zero_initialized_init_options();
                // SAFETY: Passing in a zero-initialized value is expected.
                // In the case where this returns not ok, there's nothing to clean up.
                rcl_init_options_init(&mut init_options as *mut _, allocator).ok()?;
                // SAFETY: This function does not store the ephemeral init_options and c_args
                // pointers. Passing in a zero-initialized handle is expected.
                let ret = rcl_init(
                    c_args.len() as i32,
                    if c_args.is_empty() {
                        std::ptr::null()
                    } else {
                        c_args.as_ptr()
                    },
                    &init_options as *const _,
                    handle as *mut _,
                );
                // SAFETY: It's safe to pass in an initialized object.
                // Early return will not leak memory, because this is the last fini function.
                rcl_init_options_fini(&mut init_options as *mut _).ok()?;
                // Move the check after the last fini()
                ret.ok()?;
            }
        }
        Ok(context)
    }

    pub fn create_node(&self, node_name: &str) -> Result<Node, RclReturnCode> {
        Node::new(node_name, self)
    }

    pub fn ok(&self) -> Result<bool, RclReturnCode> {
        // This will currently always return true, but once we have a signal handler, the signal
        // handler could call `rcl_shutdown()`, hence making the context invalid.
        true
    }
}

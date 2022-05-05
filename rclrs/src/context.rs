use crate::rcl_bindings::*;
use crate::{Node, RclReturnCode, ToResult};

use std::ffi::CString;
use std::os::raw::c_char;
use std::string::String;
use std::sync::Arc;
use std::vec::Vec;

use parking_lot::Mutex;

impl Drop for rcl_context_t {
    fn drop(&mut self) {
        // SAFETY: These functions have no preconditions besides a valid/initialized handle
        unsafe {
            rcl_shutdown(self);
            rcl_context_fini(self);
        }
    }
}

/// Shared state between nodes and similar entities.
///
/// It is possible, but not usually necessary, to have several contexts in an application.
///
/// Ownership of the context is shared by the `Context` itself and all nodes created from it.
///
/// # Details
/// A context stores, among other things
/// - command line arguments (used for e.g. name remapping)
/// - middleware-specific data, e.g. the domain participant in DDS
/// - the allocator used (left as the default by `rclrs`)
///
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
                rcl_init_options_init(&mut init_options, allocator).ok()?;
                // SAFETY: This function does not store the ephemeral init_options and c_args
                // pointers. Passing in a zero-initialized handle is expected.
                let ret = rcl_init(
                    c_args.len() as i32,
                    if c_args.is_empty() {
                        std::ptr::null()
                    } else {
                        c_args.as_ptr()
                    },
                    &init_options,
                    handle,
                );
                // SAFETY: It's safe to pass in an initialized object.
                // Early return will not leak memory, because this is the last fini function.
                rcl_init_options_fini(&mut init_options).ok()?;
                // Move the check after the last fini()
                ret.ok()?;
            }
        }
        Ok(context)
    }

    /// Creates a node.
    ///
    /// Convenience function equivalent to [`Node::new`][1].
    ///
    /// [1]: crate::Node::new
    ///
    /// # Example
    /// ```
    /// # use rclrs::Context;
    /// let ctx = Context::new([]).unwrap();
    /// let node = ctx.create_node("my_node");
    /// assert!(node.is_ok());
    /// ```
    pub fn create_node(&self, node_name: &str) -> Result<Node, RclReturnCode> {
        Node::new(node_name, self)
    }

    /// Creates a node in a namespace.
    ///
    /// Convenience function equivalent to [`Node::new_with_namespace`][1].
    ///
    /// [1]: crate::Node::new_with_namespace
    ///
    /// # Example
    /// ```
    /// # use rclrs::Context;
    /// let ctx = Context::new([]).unwrap();
    /// let node = ctx.create_node_with_namespace("/my/nested/namespace", "my_node");
    /// assert!(node.is_ok());
    /// ```
    pub fn create_node_with_namespace(
        &self,
        node_namespace: &str,
        node_name: &str,
    ) -> Result<Node, RclReturnCode> {
        Node::new_with_namespace(node_namespace, node_name, self)
    }

    /// Checks if the context is still valid.
    ///
    /// This will return `false` when a signal has caused the context to shut down (currently
    /// unimplemented).
    pub fn ok(&self) -> bool {
        // This will currently always return true, but once we have a signal handler, the signal
        // handler could call `rcl_shutdown()`, hence making the context invalid.
        let handle = &mut *self.handle.lock();
        // SAFETY: No preconditions for this function.
        unsafe { rcl_context_is_valid(handle) }
    }
}

#[cfg(test)]
mod tests {

    use super::*;
    use std::{env, println};

    fn default_context() -> Context {
        let args: Vec<CString> = env::args()
            .filter_map(|arg| CString::new(arg).ok())
            .collect();
        println!("<test_create_context> Context args: {:?}", args);
        Context::default(args)
    }

    #[test]
    fn test_create_context() {
        // If the context fails to be created, this will cause a panic
        let created_context = default_context();
        println!(
            "<test_create_context> Created Context: {:?}",
            created_context
        );
    }

    #[test]
    fn test_context_ok() {
        // If the context fails to be created, this will cause a panic
        let created_context = default_context();
        let ctxt_ok = created_context.ok();
        match ctxt_ok {
            Ok(is_ok) => assert!(is_ok),
            Err(err_code) => panic!(
                "<test_context_ok> RCL Error occured during test: {:?}",
                err_code
            ),
        }
    }

    #[test]
    fn test_create_node() -> Result<(), RclReturnCode> {
        // If the context fails to be created, this will cause a panic
        let created_context = default_context();
        created_context.create_node("Bob").map(|_x| ())
    }

    #[test]
    fn text_context_init() {
        // If the context fails to be created, this will cause a panic
        let args: Vec<CString> = env::args()
            .filter_map(|arg| CString::new(arg).ok())
            .collect();
        let context = Context {
            handle: Arc::new(ContextHandle(Mutex::new(unsafe {
                rcl_get_zero_initialized_context()
            }))),
        };
        context.init(args).unwrap();
    }
}

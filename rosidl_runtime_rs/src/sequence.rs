use std::cmp::Ordering;
use std::fmt::{self, Debug, Display};
use std::hash::{Hash, Hasher};
use std::iter::{Extend, FromIterator, FusedIterator};
use std::ops::{Deref, DerefMut};

#[cfg(feature = "serde")]
mod serde;

use crate::traits::SequenceAlloc;

/// An unbounded sequence.
///
/// The layout of a concrete `Sequence<T>` is the same as the corresponding `Sequence` struct
/// generated by `rosidl_generator_c`. For instance,
/// `rosidl_runtime_rs::Sequence<rosidl_runtime_rs::String>` is the same
/// as `std_msgs__msg__String__Sequence`. See the [`Message`](crate::Message) trait for background
/// information on this topic.
///
///
/// # Example
///
/// ```
/// # use rosidl_runtime_rs::{Sequence, seq};
/// let mut list = Sequence::<i32>::new(3);
/// // Sequences deref to slices
/// assert_eq!(&list[..], &[0, 0, 0]);
/// list[0] = 3;
/// list[1] = 2;
/// list[2] = 1;
/// assert_eq!(&list[..], &[3, 2, 1]);
/// // Alternatively, use the seq! macro
/// list = seq![3, 2, 1];
/// // The default sequence is empty
/// assert!(Sequence::<i32>::default().is_empty());
/// ```
#[repr(C)]
pub struct Sequence<T: SequenceAlloc> {
    data: *mut T,
    size: libc::size_t,
    capacity: libc::size_t,
}

/// A bounded sequence.
///
/// The layout of a concrete `BoundedSequence<T>` is the same as the corresponding `Sequence`
/// struct generated by `rosidl_generator_c`. For instance,
/// `rosidl_runtime_rs::BoundedSequence<rosidl_runtime_rs::String>`
/// is the same as `std_msgs__msg__String__Sequence`, which also represents both bounded
/// sequences.  See the [`Message`](crate::Message) trait for background information on this
/// topic.
///
/// # Example
///
/// ```
/// # use rosidl_runtime_rs::{BoundedSequence, seq};
/// let mut list = BoundedSequence::<i32, 5>::new(3);
/// // BoundedSequences deref to slices
/// assert_eq!(&list[..], &[0, 0, 0]);
/// list[0] = 3;
/// list[1] = 2;
/// list[2] = 1;
/// assert_eq!(&list[..], &[3, 2, 1]);
/// // Alternatively, use the seq! macro with the length specifier
/// list = seq![5 # 3, 2, 1];
/// // The default bounded sequence is empty
/// assert!(BoundedSequence::<i32, 5>::default().is_empty());
/// ```
#[derive(Clone)]
#[repr(transparent)]
pub struct BoundedSequence<T: SequenceAlloc, const N: usize> {
    inner: Sequence<T>,
}

/// Error type for [`BoundedSequence::try_new()`].
#[derive(Debug)]
pub struct SequenceExceedsBoundsError {
    len: usize,
    upper_bound: usize,
}

/// A by-value iterator created by [`Sequence::into_iter()`] and [`BoundedSequence::into_iter()`].
pub struct SequenceIterator<T: SequenceAlloc> {
    seq: Sequence<T>,
    idx: usize,
}

// ========================= impl for Sequence =========================

impl<T: SequenceAlloc> Clone for Sequence<T> {
    fn clone(&self) -> Self {
        let mut seq = Self::default();
        if T::sequence_copy(self, &mut seq) {
            seq
        } else {
            panic!("Cloning Sequence failed")
        }
    }
}

impl<T: Debug + SequenceAlloc> Debug for Sequence<T> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        self.as_slice().fmt(f)
    }
}

impl<T: SequenceAlloc> Default for Sequence<T> {
    fn default() -> Self {
        Self {
            data: std::ptr::null_mut(),
            size: 0,
            capacity: 0,
        }
    }
}

impl<T: SequenceAlloc> Deref for Sequence<T> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.as_slice()
    }
}

impl<T: SequenceAlloc> DerefMut for Sequence<T> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.as_mut_slice()
    }
}

impl<T: SequenceAlloc> Drop for Sequence<T> {
    fn drop(&mut self) {
        T::sequence_fini(self)
    }
}

impl<T: SequenceAlloc + Eq> Eq for Sequence<T> {}

impl<T: SequenceAlloc> Extend<T> for Sequence<T> {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        let it = iter.into_iter();
        // The index in the sequence where the next element will be stored
        let mut cur_idx = self.size;
        // Convenience closure for resizing self
        let resize = |seq: &mut Self, new_size: usize| {
            let old_seq = std::mem::replace(seq, Sequence::new(new_size));
            for (i, elem) in old_seq.into_iter().enumerate().take(new_size) {
                seq[i] = elem;
            }
        };
        // First, when there is a size hint > 0 (lower bound), make room for
        // that many elements.
        let num_remaining = it.size_hint().0;
        if num_remaining > 0 {
            let new_size = self.size.saturating_add(num_remaining);
            resize(self, new_size);
        }
        for item in it {
            // If there is no more capacity for the next element, resize to the
            // next power of two.
            //
            // A pedantic implementation would check for usize overflow here, but
            // that is hardly possible on real hardware. Also, not the entire
            // usize address space is usable for user space programs.
            if cur_idx == self.size {
                let new_size = (self.size + 1).next_power_of_two();
                resize(self, new_size);
            }
            self[cur_idx] = item;
            cur_idx += 1;
        }
        // All items from the iterator are stored. Shrink the sequence to fit.
        if cur_idx < self.size {
            resize(self, cur_idx);
        }
    }
}

impl<T: SequenceAlloc + Clone> From<&[T]> for Sequence<T> {
    fn from(slice: &[T]) -> Self {
        let mut seq = Sequence::new(slice.len());
        seq.clone_from_slice(slice);
        seq
    }
}

impl<T: SequenceAlloc> From<Vec<T>> for Sequence<T> {
    fn from(v: Vec<T>) -> Self {
        Sequence::from_iter(v)
    }
}

impl<T: SequenceAlloc> FromIterator<T> for Sequence<T> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let mut seq = Sequence::new(0);
        seq.extend(iter);
        seq
    }
}

impl<T: SequenceAlloc + Hash> Hash for Sequence<T> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state)
    }
}

impl<T: SequenceAlloc> IntoIterator for Sequence<T> {
    type Item = T;
    type IntoIter = SequenceIterator<T>;
    fn into_iter(self) -> Self::IntoIter {
        SequenceIterator { seq: self, idx: 0 }
    }
}

impl<T: SequenceAlloc + Ord> Ord for Sequence<T> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<T: SequenceAlloc + PartialEq> PartialEq for Sequence<T> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}

impl<T: SequenceAlloc + PartialOrd> PartialOrd for Sequence<T> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<T> Sequence<T>
where
    T: SequenceAlloc,
{
    /// Creates a sequence of `len` elements with default values.
    pub fn new(len: usize) -> Self {
        let mut seq = Self::default();
        if !T::sequence_init(&mut seq, len) {
            panic!("Sequence initialization failed");
        }
        seq
    }

    /// Extracts a slice containing the entire sequence.
    ///
    /// Equivalent to `&seq[..]`.
    pub fn as_slice(&self) -> &[T] {
        // SAFETY: self.data points to self.size consecutive, initialized elements and
        // isn't modified externally.
        unsafe { std::slice::from_raw_parts(self.data, self.size) }
    }

    /// Extracts a mutable slice containing the entire sequence.
    ///
    /// Equivalent to `&mut seq[..]`.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        // SAFETY: self.data points to self.size consecutive, initialized elements and
        // isn't modified externally.
        unsafe { std::slice::from_raw_parts_mut(self.data, self.size) }
    }
}

impl<T: Default + SequenceAlloc> Sequence<T> {
    /// Internal function for the sequence_copy impl. To be removed when rosidl#650 is backported and released.
    pub fn resize_to_at_least(&mut self, len: usize) {
        let allocation_size = std::mem::size_of::<Self>() * len;
        if self.capacity < len {
            // SAFETY: The memory in self.data is owned by C.
            let data = unsafe { libc::realloc(self.data as *mut _, allocation_size) } as *mut T;
            if data.is_null() {
                panic!("realloc failed");
            }
            // Initialize the new memory
            for i in self.capacity..len {
                // SAFETY: i is in bounds, and write() is appropriate for initializing uninitialized memory
                unsafe {
                    data.add(i).write(T::default());
                }
            }
            self.data = data;
            self.size = len;
            self.capacity = len;
        }
    }
}

// ========================= impl for BoundedSequence =========================

impl<T: Debug + SequenceAlloc, const N: usize> Debug for BoundedSequence<T, N> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        self.as_slice().fmt(f)
    }
}

impl<T: SequenceAlloc, const N: usize> Default for BoundedSequence<T, N> {
    fn default() -> Self {
        Self {
            inner: Sequence {
                data: std::ptr::null_mut(),
                size: 0,
                capacity: 0,
            },
        }
    }
}

impl<T: SequenceAlloc, const N: usize> Deref for BoundedSequence<T, N> {
    type Target = [T];
    fn deref(&self) -> &Self::Target {
        self.inner.deref()
    }
}

impl<T: SequenceAlloc, const N: usize> DerefMut for BoundedSequence<T, N> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.inner.deref_mut()
    }
}

impl<T: SequenceAlloc, const N: usize> Drop for BoundedSequence<T, N> {
    fn drop(&mut self) {
        T::sequence_fini(&mut self.inner)
    }
}

impl<T: SequenceAlloc + Eq, const N: usize> Eq for BoundedSequence<T, N> {}

impl<T: SequenceAlloc, const N: usize> Extend<T> for BoundedSequence<T, N> {
    fn extend<I>(&mut self, iter: I)
    where
        I: IntoIterator<Item = T>,
    {
        self.inner
            .extend(iter.into_iter().take(N - self.inner.size));
    }
}

impl<T: SequenceAlloc + Clone, const N: usize> TryFrom<&[T]> for BoundedSequence<T, N> {
    type Error = SequenceExceedsBoundsError;
    fn try_from(slice: &[T]) -> Result<Self, Self::Error> {
        let mut seq = BoundedSequence::try_new(slice.len())?;
        seq.clone_from_slice(slice);
        Ok(seq)
    }
}

impl<T: SequenceAlloc, const N: usize> TryFrom<Vec<T>> for BoundedSequence<T, N> {
    type Error = SequenceExceedsBoundsError;
    fn try_from(v: Vec<T>) -> Result<Self, Self::Error> {
        if v.len() > N {
            Err(SequenceExceedsBoundsError {
                len: v.len(),
                upper_bound: N,
            })
        } else {
            Ok(BoundedSequence::from_iter(v))
        }
    }
}

impl<T: SequenceAlloc, const N: usize> FromIterator<T> for BoundedSequence<T, N> {
    fn from_iter<I>(iter: I) -> Self
    where
        I: IntoIterator<Item = T>,
    {
        let mut seq = BoundedSequence::new(0);
        seq.extend(iter);
        seq
    }
}

impl<T: SequenceAlloc + Hash, const N: usize> Hash for BoundedSequence<T, N> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.as_slice().hash(state)
    }
}

impl<T: SequenceAlloc, const N: usize> IntoIterator for BoundedSequence<T, N> {
    type Item = T;
    type IntoIter = SequenceIterator<T>;
    fn into_iter(mut self) -> Self::IntoIter {
        let seq = std::mem::replace(
            &mut self.inner,
            Sequence {
                data: std::ptr::null_mut(),
                size: 0,
                capacity: 0,
            },
        );
        SequenceIterator { seq, idx: 0 }
    }
}

impl<T: SequenceAlloc + Ord, const N: usize> Ord for BoundedSequence<T, N> {
    fn cmp(&self, other: &Self) -> Ordering {
        self.as_slice().cmp(other.as_slice())
    }
}

impl<T: SequenceAlloc + PartialEq, const N: usize> PartialEq for BoundedSequence<T, N> {
    fn eq(&self, other: &Self) -> bool {
        self.as_slice().eq(other.as_slice())
    }
}

impl<T: SequenceAlloc + PartialOrd, const N: usize> PartialOrd for BoundedSequence<T, N> {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.as_slice().partial_cmp(other.as_slice())
    }
}

impl<T, const N: usize> BoundedSequence<T, N>
where
    T: SequenceAlloc,
{
    /// Creates a sequence of `len` elements with default values.
    ///
    /// If `len` is greater than `N`, this function panics.
    pub fn new(len: usize) -> Self {
        Self::try_new(len).unwrap()
    }

    /// Attempts to create a sequence of `len` elements with default values.
    ///
    /// If `len` is greater than `N`, this function returns an error.
    pub fn try_new(len: usize) -> Result<Self, SequenceExceedsBoundsError> {
        if len > N {
            return Err(SequenceExceedsBoundsError {
                len,
                upper_bound: N,
            });
        }
        let mut seq = Self::default();
        if !T::sequence_init(&mut seq.inner, len) {
            panic!("BoundedSequence initialization failed");
        }
        Ok(seq)
    }

    /// Extracts a slice containing the entire sequence.
    ///
    /// Equivalent to `&seq[..]`.
    pub fn as_slice(&self) -> &[T] {
        self.inner.as_slice()
    }

    /// Extracts a mutable slice containing the entire sequence.
    ///
    /// Equivalent to `&mut seq[..]`.
    pub fn as_mut_slice(&mut self) -> &mut [T] {
        self.inner.as_mut_slice()
    }
}

// ========================= impl for SequenceIterator =========================

impl<T: SequenceAlloc> Iterator for SequenceIterator<T> {
    type Item = T;
    fn next(&mut self) -> Option<Self::Item> {
        if self.idx >= self.seq.size {
            return None;
        }
        // SAFETY: data + idx is in bounds and points to a valid value
        let elem = unsafe {
            let ptr = self.seq.data.add(self.idx);
            let elem = ptr.read();
            // Need to make sure that dropping the sequence later will not fini() the elements
            ptr.write(std::mem::zeroed::<T>());
            elem
        };
        self.idx += 1;
        Some(elem)
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = (self.seq.size + 1) - self.idx;
        (len, Some(len))
    }
}

impl<T: SequenceAlloc> ExactSizeIterator for SequenceIterator<T> {
    fn len(&self) -> usize {
        (self.seq.size + 1) - self.idx
    }
}

impl<T: SequenceAlloc> FusedIterator for SequenceIterator<T> {}

// ========================= impl for StringExceedsBoundsError =========================

impl Display for SequenceExceedsBoundsError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> Result<(), fmt::Error> {
        write!(
            f,
            "BoundedSequence with upper bound {} initialized with len {}",
            self.upper_bound, self.len
        )
    }
}

impl std::error::Error for SequenceExceedsBoundsError {}

macro_rules! impl_sequence_alloc_for_primitive_type {
    ($rust_type:ty, $init_func:ident, $fini_func:ident, $copy_func:ident) => {
        #[link(name = "rosidl_runtime_c")]
        extern "C" {
            fn $init_func(seq: *mut Sequence<$rust_type>, size: libc::size_t) -> bool;
            fn $fini_func(seq: *mut Sequence<$rust_type>);
        }

        impl SequenceAlloc for $rust_type {
            fn sequence_init(seq: &mut Sequence<Self>, size: libc::size_t) -> bool {
                // SAFETY: There are no special preconditions to the sequence_init function.
                unsafe {
                    // This allocates space and sets seq.size and seq.capacity to size
                    let ret = $init_func(seq as *mut _, size);
                    // Zero memory, since it will be uninitialized if there is no default value
                    std::ptr::write_bytes(seq.data, 0u8, size);
                    ret
                }
            }
            fn sequence_fini(seq: &mut Sequence<Self>) {
                // SAFETY: There are no special preconditions to the sequence_fini function.
                unsafe { $fini_func(seq as *mut _) }
            }
            fn sequence_copy(in_seq: &Sequence<Self>, out_seq: &mut Sequence<Self>) -> bool {
                let allocation_size = std::mem::size_of::<Self>() * in_seq.size;
                if out_seq.capacity < in_seq.size {
                    // SAFETY: The memory in out_seq.data is owned by C.
                    let data = unsafe { libc::realloc(out_seq.data as *mut _, allocation_size) };
                    if data.is_null() {
                        return false;
                    }
                    out_seq.data = data as *mut _;
                    out_seq.capacity = in_seq.size;
                }
                // SAFETY: The memory areas don't overlap.
                unsafe {
                    libc::memcpy(
                        out_seq.data as *mut _,
                        in_seq.data as *const _,
                        allocation_size,
                    );
                }
                out_seq.size = in_seq.size;
                true
            }
        }
    };
}

// Primitives are not messages themselves, but there can be sequences of them.
//
// See https://github.com/ros2/rosidl/blob/master/rosidl_runtime_c/include/rosidl_runtime_c/primitives_sequence.h
// Long double isn't available in Rust, so it is skipped.
impl_sequence_alloc_for_primitive_type!(
    f32,
    rosidl_runtime_c__float__Sequence__init,
    rosidl_runtime_c__float__Sequence__fini,
    rosidl_runtime_c__float__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    f64,
    rosidl_runtime_c__double__Sequence__init,
    rosidl_runtime_c__double__Sequence__fini,
    rosidl_runtime_c__double__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    bool,
    rosidl_runtime_c__boolean__Sequence__init,
    rosidl_runtime_c__boolean__Sequence__fini,
    rosidl_runtime_c__boolean__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    u8,
    rosidl_runtime_c__uint8__Sequence__init,
    rosidl_runtime_c__uint8__Sequence__fini,
    rosidl_runtime_c__uint8__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    i8,
    rosidl_runtime_c__int8__Sequence__init,
    rosidl_runtime_c__int8__Sequence__fini,
    rosidl_runtime_c__int8__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    u16,
    rosidl_runtime_c__uint16__Sequence__init,
    rosidl_runtime_c__uint16__Sequence__fini,
    rosidl_runtime_c__uint16__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    i16,
    rosidl_runtime_c__int16__Sequence__init,
    rosidl_runtime_c__int16__Sequence__fini,
    rosidl_runtime_c__int16__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    u32,
    rosidl_runtime_c__uint32__Sequence__init,
    rosidl_runtime_c__uint32__Sequence__fini,
    rosidl_runtime_c__uint32__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    i32,
    rosidl_runtime_c__int32__Sequence__init,
    rosidl_runtime_c__int32__Sequence__fini,
    rosidl_runtime_c__int32__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    u64,
    rosidl_runtime_c__uint64__Sequence__init,
    rosidl_runtime_c__uint64__Sequence__fini,
    rosidl_runtime_c__uint64__Sequence__copy
);
impl_sequence_alloc_for_primitive_type!(
    i64,
    rosidl_runtime_c__int64__Sequence__init,
    rosidl_runtime_c__int64__Sequence__fini,
    rosidl_runtime_c__int64__Sequence__copy
);

/// Creates a sequence, similar to the `vec!` macro.
///
/// It's possible to create both [`Sequence`]s and [`BoundedSequence`]s.
/// Unbounded sequences are created by a comma-separated list of values.
/// Bounded sequences are created by additionally specifying the maximum capacity (the `N` type
/// parameter) in the beginning, followed by a `#`.
///
/// # Example
/// ```
/// # use rosidl_runtime_rs::{BoundedSequence, Sequence, seq};
/// let unbounded: Sequence<i32> = seq![1, 2, 3];
/// let bounded: BoundedSequence<i32, 5> = seq![5 # 1, 2, 3];
/// assert_eq!(&unbounded[..], &bounded[..])
/// ```
#[macro_export]
macro_rules! seq {
    [$( $elem:expr ),*] => {
        {
            let len = seq!(@count_tts $($elem),*);
            let mut seq = Sequence::new(len);
            let mut i = 0;
            $(
                seq[i] = $elem;
                #[allow(unused_assignments)]
                { i += 1; }
            )*
            seq
        }
    };
    [$len:literal # $( $elem:expr ),*] => {
        {
            let len = seq!(@count_tts $($elem),*);
            let mut seq = BoundedSequence::<_, $len>::new(len);
            let mut i = 0;
            $(
                seq[i] = $elem;
                #[allow(unused_assignments)]
                { i += 1; }
            )*
            seq
        }
    };
    // https://danielkeep.github.io/tlborm/book/blk-counting.html
    (@replace_expr ($_t:expr, $sub:expr)) => {$sub};
    (@count_tts $($e:expr),*) => {<[()]>::len(&[$(seq!(@replace_expr ($e, ()))),*])};
}

#[cfg(test)]
mod tests {
    use super::*;
    use quickcheck::{quickcheck, Arbitrary, Gen};

    impl<T: Arbitrary + SequenceAlloc> Arbitrary for Sequence<T> {
        fn arbitrary(g: &mut Gen) -> Self {
            Vec::arbitrary(g).into()
        }
    }

    impl<T: Arbitrary + SequenceAlloc> Arbitrary for BoundedSequence<T, 256> {
        fn arbitrary(g: &mut Gen) -> Self {
            let len = u8::arbitrary(g);
            (0..len).map(|_| T::arbitrary(g)).collect()
        }
    }

    quickcheck! {
        fn test_extend(xs: Vec<i32>, ys: Vec<i32>) -> bool {
            let mut xs_seq = Sequence::new(xs.len());
            xs_seq.copy_from_slice(&xs);
            xs_seq.extend(ys.clone());
            if xs_seq.len() != xs.len() + ys.len() {
                return false;
            }
            if xs_seq[..xs.len()] != xs[..] {
                return false;
            }
            if xs_seq[xs.len()..] != ys[..] {
                return false;
            }
            true
        }
    }

    quickcheck! {
        fn test_iteration(xs: Vec<i32>) -> bool {
            let mut seq_1 = Sequence::new(xs.len());
            seq_1.copy_from_slice(&xs);
            let seq_2 = seq_1.clone().into_iter().collect();
            seq_1 == seq_2
        }
    }
}

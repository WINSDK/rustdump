#![allow(dead_code, unused_variables, unused_assignments)]

use std::fmt;
use std::mem::MaybeUninit;
use std::sync::atomic::{AtomicUsize, Ordering};

pub mod arm;
pub mod riscv;
pub mod x86_64;

mod lookup;

#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum BitWidth {
    U128,
    U64,
    U32,
    U16,
}

pub struct Array<T, const S: usize> {
    bytes: [T; S],
    len: AtomicUsize,
}

impl<T: Default, const S: usize> Array<T, S> {
    pub fn new() -> Self {
        let mut bytes: MaybeUninit<[T; S]> = MaybeUninit::uninit();
        let mut ptr = bytes.as_mut_ptr() as *mut T;

        for _ in 0..S {
            unsafe {
                ptr.write(T::default());
                ptr = ptr.offset(1);
            }
        }

        Self { bytes: unsafe { bytes.assume_init() }, len: AtomicUsize::new(0) }
    }
}

impl<T, const S: usize> Array<T, S> {
    pub fn len(&self) -> usize {
        self.len.load(Ordering::Relaxed)
    }

    pub fn remove(&mut self, idx: usize) {
        let len = self.len.load(Ordering::Acquire);
        assert!(idx < len, "idx is {idx} which is out of bounce for len of {len}");

        unsafe {
            std::ptr::copy(
                self.bytes.as_ptr().add(idx + 1),
                self.bytes.as_mut_ptr().add(idx),
                len - idx - 1,
            );
        }

        self.len.store(len - 1, Ordering::Release);
    }
}

impl<T: Clone, const S: usize> Clone for Array<T, S> {
    fn clone(&self) -> Self {
        Self { bytes: self.bytes.clone(), len: AtomicUsize::new(self.len.load(Ordering::SeqCst)) }
    }
}

impl<T: PartialEq, const S: usize> PartialEq for Array<T, S> {
    fn eq(&self, other: &Self) -> bool {
        self.bytes == other.bytes
    }
}

impl<T: Eq, const S: usize> Eq for Array<T, S> {}

impl<T: Default + Copy, const S: usize> Default for Array<T, S> {
    fn default() -> Self {
        Self { bytes: [T::default(); S], len: AtomicUsize::new(0) }
    }
}

impl<T, const S: usize> std::ops::Index<usize> for Array<T, S> {
    type Output = T;

    #[inline]
    fn index(&self, idx: usize) -> &Self::Output {
        self.len.fetch_max(idx + 1, Ordering::AcqRel);
        &self.bytes[idx]
    }
}

impl<T, const S: usize> std::ops::IndexMut<usize> for Array<T, S> {
    #[inline]
    fn index_mut(&mut self, idx: usize) -> &mut Self::Output {
        self.len.fetch_max(idx + 1, Ordering::AcqRel);
        &mut self.bytes[idx]
    }
}

impl<T, const S: usize> AsRef<[T]> for Array<T, S> {
    fn as_ref(&self) -> &[T] {
        let len = self.len();
        &self.bytes[..len]
    }
}

impl<T, const S: usize> AsMut<[T]> for Array<T, S> {
    fn as_mut(&mut self) -> &mut [T] {
        let len = self.len();
        &mut self.bytes[..len]
    }
}

impl<T: fmt::Debug, const S: usize> fmt::Debug for Array<T, S> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", &self.bytes[..self.len.load(Ordering::Relaxed)])
    }
}

pub struct Reader<'a> {
    pub buf: &'a [u8],
    pub pos: AtomicUsize,
}

impl<'a> Reader<'a> {
    pub fn new(buf: &'a [u8]) -> Self {
        Self { buf, pos: AtomicUsize::new(0) }
    }

    pub fn inner(&self) -> &'a [u8] {
        &self.buf[self.pos.load(Ordering::SeqCst)..]
    }

    pub fn offset(&self, num_bytes: isize) {
        let pos = self.pos.load(Ordering::Acquire) as isize;
        self.pos.store((pos + num_bytes) as usize, Ordering::Release);
    }

    pub fn take(&self, byte: u8) -> bool {
        let pos = self.pos.load(Ordering::Acquire);
        if self.buf.get(pos) == Some(&byte) {
            self.pos.store(pos + 1, Ordering::Release);
            true
        } else {
            false
        }
    }

    pub fn take_slice(&self, bytes: &[u8]) -> bool {
        let pos = self.pos.load(Ordering::Acquire);
        if self.buf.get(pos..pos + bytes.len()) == Some(bytes) {
            self.pos.store(pos + bytes.len(), Ordering::Release);
            true
        } else {
            false
        }
    }

    #[inline]
    pub fn consume(&self) -> Option<u8> {
        let pos = self.pos.fetch_add(1, Ordering::AcqRel);
        self.buf.get(pos).copied()
    }

    /// Returns `None` if either the reader is at the end of a byte stream or the conditional
    /// fails, on success will increment internal position.
    pub fn consume_eq<F: FnOnce(u8) -> bool>(&self, f: F) -> Option<u8> {
        let pos = self.pos.load(Ordering::Acquire);
        self.buf.get(pos).filter(|x| f(**x)).map(|x| {
            self.pos.store(pos + 1, Ordering::Release);
            *x
        })
    }
}

unsafe impl Send for Reader<'_> {}
unsafe impl Sync for Reader<'_> {}

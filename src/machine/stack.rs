use crate::machine::value::MachineValue;

/// Slots allocated up front, enough that most programs never grow the stack.
const INITIAL_CAPACITY: usize = 64;

/// A stack of machine values, laid out so generated code can push and pop
/// without calling a helper: `len`, `capacity`, and `data` sit at fixed offsets
/// a `repr(C)` layout pins down, so a push is a bounds compare against
/// `capacity`, a store through `data`, and a write-back of the incremented
/// `len`.
///
/// `store` owns the buffer and its length is always `capacity`, so every slot is
/// initialized and the ones at or above `len` merely hold values already popped.
/// `MachineValue` is `Copy` with no drop glue, so leaving them there is
/// harmless, and it means growing never has to move initialized values around.
///
/// `data` and `capacity` mirror `store`, so both are re-derived by `sync`
/// wherever the buffer can move: growth, and cloning.
#[repr(C)]
pub struct ValueStack {
    /// Live values occupy `0..len`, the bottom of the stack being index zero.
    len: usize,
    /// Slots in `store` available before it has to grow.
    capacity: usize,
    /// `store`'s buffer.
    data: *mut MachineValue,
    store: Vec<MachineValue>,
}

impl ValueStack {
    pub(crate) fn new() -> Self {
        let mut stack = Self {
            len: 0,
            capacity: 0,
            data: std::ptr::null_mut(),
            store: vec![MachineValue::None; INITIAL_CAPACITY],
        };
        stack.sync();
        stack
    }

    /// Re-reads the buffer's address and length after `store` may have moved.
    fn sync(&mut self) {
        self.capacity = self.store.len();
        self.data = self.store.as_mut_ptr();
    }

    /// Doubles the buffer. Kept out of line so the common push stays small.
    #[cold]
    fn grow(&mut self) {
        let capacity = (self.capacity * 2).max(INITIAL_CAPACITY);
        self.store.resize(capacity, MachineValue::None);
        self.sync();
    }

    #[inline]
    pub(crate) fn push(&mut self, value: MachineValue) {
        if self.len == self.capacity {
            self.grow();
        }
        self.store[self.len] = value;
        self.len += 1;
    }

    #[inline]
    pub(crate) fn pop(&mut self) -> Option<MachineValue> {
        let index = self.len.checked_sub(1)?;
        self.len = index;
        Some(self.store[index])
    }

    /// Drops every value but keeps the buffer, so a reused machine does not pay
    /// to grow again.
    pub(crate) fn clear(&mut self) {
        self.len = 0;
    }

    pub(crate) fn len(&self) -> usize {
        self.len
    }

    /// The live values, deepest first.
    pub(crate) fn values(&self) -> &[MachineValue] {
        &self.store[..self.len]
    }

    /// Byte offset of `len` from the start of the stack, for generated code.
    pub(crate) const fn length_offset() -> usize {
        std::mem::offset_of!(ValueStack, len)
    }

    /// Byte offset of `capacity` from the start of the stack.
    pub(crate) const fn capacity_offset() -> usize {
        std::mem::offset_of!(ValueStack, capacity)
    }

    /// Byte offset of the buffer pointer from the start of the stack.
    pub(crate) const fn data_offset() -> usize {
        std::mem::offset_of!(ValueStack, data)
    }
}

impl Default for ValueStack {
    fn default() -> Self {
        Self::new()
    }
}

impl Clone for ValueStack {
    fn clone(&self) -> Self {
        let mut stack = Self {
            len: self.len,
            capacity: 0,
            data: std::ptr::null_mut(),
            store: self.store.clone(),
        };
        stack.sync();
        stack
    }
}

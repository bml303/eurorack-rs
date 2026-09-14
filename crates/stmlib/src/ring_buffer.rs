//! `stmlib/utils/ring_buffer.h` -- basic ring buffer template. Only the
//! non-blocking subset used by the ports (`Init`, `writable`/`readable`,
//! `Overwrite`, `ImmediateRead` on single elements) is implemented; the C
//! class also has spin-waiting `Write`/`Read` and bulk slice variants that no
//! current port needs.

#[derive(Debug, Clone)]
pub struct RingBuffer<T, const N: usize> {
    buffer: [T; N],
    read_ptr: usize,
    write_ptr: usize,
}

impl<T: Default + Copy, const N: usize> Default for RingBuffer<T, N> {
    fn default() -> Self {
        Self {
            buffer: [T::default(); N],
            read_ptr: 0,
            write_ptr: 0,
        }
    }
}

impl<T: Copy, const N: usize> RingBuffer<T, N> {
    pub fn init(&mut self) {
        self.read_ptr = 0;
        self.write_ptr = 0;
    }

    #[inline]
    pub fn capacity(&self) -> usize {
        N
    }

    #[inline]
    pub fn writable(&self) -> usize {
        (N + self.read_ptr - self.write_ptr - 1) % N
    }

    #[inline]
    pub fn readable(&self) -> usize {
        (N + self.write_ptr - self.read_ptr) % N
    }

    #[inline]
    pub fn overwrite(&mut self, v: T) {
        let w = self.write_ptr;
        self.buffer[w] = v;
        self.write_ptr = (w + 1) % N;
    }

    #[inline]
    pub fn immediate_read(&mut self) -> T {
        let r = self.read_ptr;
        let result = self.buffer[r];
        self.read_ptr = (r + 1) % N;
        result
    }

    #[inline]
    pub fn flush(&mut self) {
        self.write_ptr = self.read_ptr;
    }
}

//! `stmlib/algorithms/voice_allocator.h` -- polyphonic voice allocator.

pub const NOT_ALLOCATED: u8 = 0xff;
const ACTIVE_NOTE: u8 = 0x80;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum VoiceStealingMode {
    Lru,
    Mru,
}

#[derive(Debug, Clone)]
pub struct VoiceAllocator<const CAPACITY: usize> {
    pool: [u8; CAPACITY],
    /// Holds the indices of the voices sorted by most recent usage.
    lru: [u8; CAPACITY],
    size: u8,
}

impl<const CAPACITY: usize> Default for VoiceAllocator<CAPACITY> {
    fn default() -> Self {
        Self {
            pool: [0; CAPACITY],
            lru: [0; CAPACITY],
            size: 0,
        }
    }
}

impl<const CAPACITY: usize> VoiceAllocator<CAPACITY> {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn init(&mut self) {
        self.size = 0;
        self.clear();
    }

    pub fn note_on(&mut self, note: u8) -> u8 {
        self.note_on_with_mode(note, VoiceStealingMode::Lru)
    }

    pub fn note_on_with_mode(&mut self, note: u8, voice_stealing_mode: VoiceStealingMode) -> u8 {
        if self.size == 0 {
            return NOT_ALLOCATED;
        }

        // First, check if there is a voice currently playing this note. In
        // this case, this voice will be responsible for retriggering this
        // note. Hint: if you're more into string instruments than keyboard
        // instruments, you can safely comment those lines.
        let mut voice = self.find(note);

        // Then, try to find the least recently touched, currently inactive
        // voice.
        if voice == NOT_ALLOCATED {
            for i in 0..CAPACITY {
                let candidate = self.lru[i];
                if candidate < self.size && self.pool[candidate as usize] & ACTIVE_NOTE == 0 {
                    voice = candidate;
                }
            }
        }
        // If all voices are active, use the least or most recently played
        // note (voice-stealing).
        if voice == NOT_ALLOCATED {
            for i in 0..CAPACITY {
                let candidate_idx = if voice_stealing_mode == VoiceStealingMode::Lru {
                    i
                } else {
                    CAPACITY - 1 - i
                };
                let candidate = self.lru[candidate_idx];
                if candidate < self.size {
                    voice = candidate;
                }
            }
        }
        self.pool[voice as usize] = note | ACTIVE_NOTE;
        self.touch(voice);
        voice
    }

    pub fn note_off(&mut self, note: u8) -> u8 {
        let voice = self.find(note);
        if voice != NOT_ALLOCATED {
            self.pool[voice as usize] &= 0x7f;
            self.touch(voice);
        }
        voice
    }

    pub fn find(&self, note: u8) -> u8 {
        for i in 0..self.size {
            if self.pool[i as usize] & 0x7f == note {
                return i;
            }
        }
        NOT_ALLOCATED
    }

    pub fn clear(&mut self) {
        self.pool = [0; CAPACITY];
        for i in 0..CAPACITY {
            self.lru[i] = (CAPACITY - i - 1) as u8;
        }
    }

    pub fn clear_notes(&mut self) {
        for slot in self.pool.iter_mut() {
            *slot &= 0x7f;
        }
    }

    pub fn set_size(&mut self, size: u8) {
        self.size = size;
    }

    pub fn size(&self) -> u8 {
        self.size
    }

    fn touch(&mut self, voice: u8) {
        let mut source = CAPACITY as i32 - 1;
        let mut destination = CAPACITY as i32 - 1;
        while source >= 0 {
            if self.lru[source as usize] != voice {
                self.lru[destination as usize] = self.lru[source as usize];
                destination -= 1;
            }
            source -= 1;
        }
        self.lru[0] = voice;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn allocates_distinct_voices_lru_first() {
        let mut alloc: VoiceAllocator<4> = VoiceAllocator::new();
        alloc.init();
        alloc.set_size(4);

        let v1 = alloc.note_on(60);
        let v2 = alloc.note_on(64);
        let v3 = alloc.note_on(67);
        assert_ne!(v1, v2);
        assert_ne!(v2, v3);
        assert_ne!(v1, v3);
    }

    #[test]
    fn retriggers_same_voice_for_a_held_note() {
        let mut alloc: VoiceAllocator<4> = VoiceAllocator::new();
        alloc.init();
        alloc.set_size(4);

        let v1 = alloc.note_on(60);
        alloc.note_off(60);
        let v2 = alloc.note_on(60);
        assert_eq!(v1, v2);
    }

    #[test]
    fn steals_a_voice_once_all_are_active() {
        let mut alloc: VoiceAllocator<2> = VoiceAllocator::new();
        alloc.init();
        alloc.set_size(2);

        alloc.note_on(60);
        alloc.note_on(64);
        // Both voices are active; a third note must steal one rather than
        // returning NOT_ALLOCATED.
        let v3 = alloc.note_on(67);
        assert_ne!(v3, NOT_ALLOCATED);
    }

    #[test]
    fn no_voices_available_when_size_is_zero() {
        let mut alloc: VoiceAllocator<4> = VoiceAllocator::new();
        alloc.init();
        assert_eq!(alloc.note_on(60), NOT_ALLOCATED);
    }
}

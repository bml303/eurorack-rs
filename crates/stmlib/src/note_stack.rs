//! `stmlib/algorithms/note_stack.h` -- stack of currently pressed keys.
//!
//! Currently pressed keys are stored as a linked list, used as a LIFO stack
//! to allow monosynth-like behaviour: press and hold C4 -> C4 plays; also
//! hold C5 -> C5 plays; also hold G4 -> G4 plays; release G4 -> C5 plays
//! again; release C5 -> C4 plays again.
//!
//! The nodes used in the linked list are pre-allocated from a pool of `N`
//! nodes, so the "pointers" (to the root element, for example) are plain
//! indices into the pool, not real references. An array of indices is also
//! kept sorted by ascending pitch, for arpeggiation.
//!
//! The C template is `NoteStack<capacity>`, backed by a `capacity + 1`-sized
//! pool (`pool_[0]` is a dummy node). Rust const generics can't express that
//! arithmetic on stable, so `POOL_SIZE` here is that `capacity + 1` already
//! applied (the sole instantiation, `NoteStack<12>`, becomes
//! `NoteStack<13>`) -- the same trick used by `PatternPredictor`.

#[derive(Debug, Clone, Copy, Default)]
pub struct NoteEntry {
    pub note: u8,
    pub velocity: u8,
    next_ptr: u8, // base 1.
}

pub const FREE_SLOT: u8 = 0xff;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NoteStackPriority {
    Last,
    Low,
    High,
    First,
}

#[derive(Debug, Clone)]
pub struct NoteStack<const POOL_SIZE: usize> {
    size: u8,
    pool: [NoteEntry; POOL_SIZE],
    root_ptr: u8, // base 1.
    sorted_ptr: [u8; POOL_SIZE],
}

impl<const POOL_SIZE: usize> Default for NoteStack<POOL_SIZE> {
    fn default() -> Self {
        let mut s = Self {
            size: 0,
            pool: [NoteEntry::default(); POOL_SIZE],
            root_ptr: 0,
            sorted_ptr: [0; POOL_SIZE],
        };
        s.clear();
        s
    }
}

impl<const POOL_SIZE: usize> NoteStack<POOL_SIZE> {
    pub fn new() -> Self {
        Self::default()
    }

    #[inline]
    fn capacity_u8(&self) -> u8 {
        (POOL_SIZE - 1) as u8
    }

    pub fn init(&mut self) {
        self.clear();
    }

    pub fn note_on(&mut self, note: u8, velocity: u8) {
        let capacity = self.capacity_u8();
        // Remove the note from the list first (in case it is already here).
        self.note_off(note);
        // In case of saturation, remove the least recently played note from
        // the stack.
        if self.size == capacity {
            let mut least_recent_note = 1u8;
            for i in 1..=capacity {
                if self.pool[i as usize].next_ptr == 0 {
                    least_recent_note = self.pool[i as usize].note;
                }
            }
            self.note_off(least_recent_note);
        }
        // Now we are ready to insert the new note. Find a free slot to
        // insert it.
        let mut free_slot = 1u8;
        for i in 1..=capacity {
            if self.pool[i as usize].note == FREE_SLOT {
                free_slot = i;
                break;
            }
        }
        self.pool[free_slot as usize].next_ptr = self.root_ptr;
        self.pool[free_slot as usize].note = note;
        self.pool[free_slot as usize].velocity = velocity;
        self.root_ptr = free_slot;
        // The last step consists in inserting the note in the sorted list.
        for i in 0..self.size {
            if self.pool[self.sorted_ptr[i as usize] as usize].note > note {
                let mut j = self.size;
                while j > i {
                    self.sorted_ptr[j as usize] = self.sorted_ptr[(j - 1) as usize];
                    j -= 1;
                }
                self.sorted_ptr[i as usize] = free_slot;
                free_slot = 0;
                break;
            }
        }
        if free_slot != 0 {
            self.sorted_ptr[self.size as usize] = free_slot;
        }
        self.size += 1;
    }

    pub fn note_off(&mut self, note: u8) {
        let mut current = self.root_ptr;
        let mut previous = 0u8;
        while current != 0 {
            if self.pool[current as usize].note == note {
                break;
            }
            previous = current;
            current = self.pool[current as usize].next_ptr;
        }
        if current != 0 {
            if previous != 0 {
                self.pool[previous as usize].next_ptr = self.pool[current as usize].next_ptr;
            } else {
                self.root_ptr = self.pool[current as usize].next_ptr;
            }
            for i in 0..self.size {
                if self.sorted_ptr[i as usize] == current {
                    let mut j = i;
                    while j < self.size - 1 {
                        self.sorted_ptr[j as usize] = self.sorted_ptr[(j + 1) as usize];
                        j += 1;
                    }
                    break;
                }
            }
            self.pool[current as usize].next_ptr = 0;
            self.pool[current as usize].note = FREE_SLOT;
            self.pool[current as usize].velocity = 0;
            self.size -= 1;
        }
    }

    pub fn clear(&mut self) {
        self.size = 0;
        for slot in self.pool.iter_mut() {
            *slot = NoteEntry::default();
        }
        for slot in self.sorted_ptr.iter_mut() {
            *slot = 0;
        }
        self.root_ptr = 0;
        for slot in self.pool.iter_mut() {
            slot.note = FREE_SLOT;
        }
    }

    pub fn size(&self) -> u8 {
        self.size
    }

    pub fn max_size(&self) -> u8 {
        self.capacity_u8()
    }

    pub fn most_recent_note(&self) -> &NoteEntry {
        &self.pool[self.root_ptr as usize]
    }

    pub fn least_recent_note(&self) -> &NoteEntry {
        let mut current = self.root_ptr;
        while current != 0 && self.pool[current as usize].next_ptr != 0 {
            current = self.pool[current as usize].next_ptr;
        }
        &self.pool[current as usize]
    }

    pub fn played_note(&self, index: u8) -> &NoteEntry {
        let mut current = self.root_ptr;
        let index = self.size - index - 1;
        for _ in 0..index {
            current = self.pool[current as usize].next_ptr;
        }
        &self.pool[current as usize]
    }

    pub fn sorted_note(&self, index: u8) -> &NoteEntry {
        &self.pool[self.sorted_ptr[index as usize] as usize]
    }

    pub fn note(&self, index: u8) -> &NoteEntry {
        &self.pool[index as usize]
    }

    pub fn mutable_note(&mut self, index: u8) -> &mut NoteEntry {
        &mut self.pool[index as usize]
    }

    pub fn dummy(&self) -> &NoteEntry {
        &self.pool[0]
    }

    pub fn note_by_priority(&self, priority: NoteStackPriority, index: u8) -> &NoteEntry {
        if self.size() <= index {
            return self.dummy();
        }
        match priority {
            NoteStackPriority::Last => self.played_note(self.size() - 1 - index),
            NoteStackPriority::Low => self.sorted_note(index),
            NoteStackPriority::High => self.sorted_note(self.size() - 1 - index),
            NoteStackPriority::First => self.played_note(index),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lifo_monosynth_scenario() {
        // Press and hold C4 -> C4 plays. Also hold C5 -> C5 plays. Also
        // hold G4 -> G4 plays. Release G4 -> C5 plays again. Release C5 ->
        // C4 plays again.
        let mut stack: NoteStack<13> = NoteStack::new();
        stack.init();

        stack.note_on(60, 100); // C4
        assert_eq!(stack.most_recent_note().note, 60);

        stack.note_on(72, 100); // C5
        assert_eq!(stack.most_recent_note().note, 72);

        stack.note_on(67, 100); // G4
        assert_eq!(stack.most_recent_note().note, 67);

        stack.note_off(67);
        assert_eq!(stack.most_recent_note().note, 72);

        stack.note_off(72);
        assert_eq!(stack.most_recent_note().note, 60);

        assert_eq!(stack.size(), 1);
    }

    #[test]
    fn sorted_order_by_pitch() {
        let mut stack: NoteStack<13> = NoteStack::new();
        stack.init();
        for &note in &[67u8, 60, 72, 64] {
            stack.note_on(note, 100);
        }
        let sorted: [u8; 4] = [
            stack.sorted_note(0).note,
            stack.sorted_note(1).note,
            stack.sorted_note(2).note,
            stack.sorted_note(3).note,
        ];
        assert_eq!(sorted, [60, 64, 67, 72]);
    }

    #[test]
    fn saturation_evicts_least_recently_played() {
        let mut stack: NoteStack<3> = NoteStack::new();
        stack.init();
        stack.note_on(10, 1);
        stack.note_on(20, 1);
        // Capacity 2: this should evict note 10.
        stack.note_on(30, 1);
        assert_eq!(stack.size(), 2);
        assert_eq!(stack.least_recent_note().note, 20);
    }
}

//! A user wavetable for Braids' wavetable shapes (WTBL, WMAP, WLIN, WTx4):
//! custom waves in place of some of the 256 built-in ones (`WT_WAVES`),
//! which all four shapes then play wherever they would play those, and
//! optionally a map of the wave map shape's 16 x 16 waves (`WT_MAP`).
//!
//! Not in the C (Braids has no user data); given with
//! [`MacroOscillator::set_user_wavetable`](crate::MacroOscillator::set_user_wavetable).
//! Without one the shapes play exactly as the C.
//!
//! # The file ([`UserWavetable::from_bytes`])
//!
//! | Bytes | What |
//! | :--- | :--- |
//! | 4 | `"BRWT"` |
//! | 1 | version, 1 |
//! | 1 | `n`, the custom waves (0..=[`MAX_CUSTOM_WAVES`]) |
//! | 1 | flags: bit 0, a map follows the slots |
//! | 1 | 0 |
//! | `n` | each custom wave's slot: the built-in wave (0..256) it replaces, each at most once |
//! | 256 | with flag bit 0: the map, `map[x * 16 + y]` a wave (0..256, after the replacing) for TIMBRE `x` and COLOR `y` |
//! | `n` × 129 | the waves: 128 unsigned 8-bit samples (128 the middle), then the first again |
//!
//! At most 4034 bytes, so it fits the 4 KB the other user data files take.

use crate::resources::{WT_MAP, WT_WAVES};

/// Waves of the built-in pool, and the samples of one
pub const NUM_WAVES: usize = 256;
pub const WAVE_SIZE: usize = 129;
/// Entries of the wave map shape's map
pub const MAP_SIZE: usize = 256;
/// Custom waves a user wavetable holds at most
pub const MAX_CUSTOM_WAVES: usize = 29;

const MAGIC: &[u8; 4] = b"BRWT";
const VERSION: u8 = 1;
const HEADER: usize = 8;
const FLAG_MAP: u8 = 1;

/// Braids' wave pool with some waves replaced, and the wave map's map
#[derive(Debug, Clone, PartialEq)]
pub struct UserWavetable {
    /// Per built-in wave: 0, itself; `i + 1`, custom wave `i`
    replaced: [u8; NUM_WAVES],
    map: [u8; MAP_SIZE],
    waves: [u8; MAX_CUSTOM_WAVES * WAVE_SIZE],
    count: usize,
}

impl Default for UserWavetable {
    fn default() -> Self {
        Self::new()
    }
}

impl UserWavetable {
    /// The largest file: every custom wave and the map
    pub const MAX_SIZE: usize = HEADER + MAX_CUSTOM_WAVES + MAP_SIZE + MAX_CUSTOM_WAVES * WAVE_SIZE;

    /// The built-in waves and map: plays as without a user wavetable
    pub const fn new() -> Self {
        UserWavetable { replaced: [0; NUM_WAVES], map: WT_MAP, waves: [0; MAX_CUSTOM_WAVES * WAVE_SIZE], count: 0 }
    }

    /// Replace built-in wave `slot` with `wave`; false when `MAX_CUSTOM_WAVES`
    /// other slots are replaced already
    pub fn set_wave(&mut self, slot: u8, wave: &[u8; WAVE_SIZE]) -> bool {
        let i = match self.replaced[slot as usize] {
            0 if self.count == MAX_CUSTOM_WAVES => return false,
            0 => {
                self.count += 1;
                self.replaced[slot as usize] = self.count as u8;
                self.count - 1
            }
            r => r as usize - 1,
        };
        self.waves[i * WAVE_SIZE..(i + 1) * WAVE_SIZE].copy_from_slice(wave);
        true
    }

    /// The wave map shape's map (`map[x * 16 + y]`, waves 0..256)
    pub fn set_map(&mut self, map: &[u8; MAP_SIZE]) {
        self.map = *map;
    }

    /// Custom waves held
    pub fn custom_waves(&self) -> usize {
        self.count
    }

    /// From a file (see the module); `None` when it is not one: a wrong
    /// header or size, or a slot twice
    pub fn from_bytes(bytes: &[u8]) -> Option<UserWavetable> {
        let header = bytes.get(..HEADER)?;
        let (n, flags) = (header[5] as usize, header[6]);
        if &header[..4] != MAGIC || header[4] != VERSION || n > MAX_CUSTOM_WAVES || flags & !FLAG_MAP != 0 {
            return None;
        }
        let map_size = if flags & FLAG_MAP != 0 { MAP_SIZE } else { 0 };
        if bytes.len() != HEADER + n + map_size + n * WAVE_SIZE {
            return None;
        }
        let slots = &bytes[HEADER..HEADER + n];
        let waves = &bytes[HEADER + n + map_size..];
        let mut table = UserWavetable::new();
        for (i, &slot) in slots.iter().enumerate() {
            if table.replaced[slot as usize] != 0 {
                return None;
            }
            table.set_wave(slot, waves[i * WAVE_SIZE..(i + 1) * WAVE_SIZE].try_into().unwrap());
        }
        if map_size != 0 {
            table.map.copy_from_slice(&bytes[HEADER + n..HEADER + n + MAP_SIZE]);
        }
        Some(table)
    }

    /// The samples of wave `w` (0..256): the custom one replacing it or the
    /// built-in one
    #[inline]
    pub fn wave(&self, w: usize) -> &[u8] {
        match self.replaced[w] as usize {
            0 => &WT_WAVES[w * WAVE_SIZE..(w + 1) * WAVE_SIZE],
            r => &self.waves[(r - 1) * WAVE_SIZE..r * WAVE_SIZE],
        }
    }

    /// The wave the map names at `i` (`x * 16 + y`)
    #[inline]
    pub fn map(&self, i: usize) -> usize {
        self.map[i] as usize
    }
}

/// The built-in pool and map, as a user wavetable (what the shapes play
/// without one)
pub(crate) static BUILT_IN: UserWavetable = UserWavetable::new();

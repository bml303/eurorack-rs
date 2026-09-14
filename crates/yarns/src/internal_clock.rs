//! `yarns/internal_clock.h` -- the internal MIDI clock, with swing.

#[derive(Debug, Clone, Copy, Default)]
pub struct InternalClock {
    phase: u32,
    phase_increment: u32,
    swing_amount: u32,
    swing_step: u8,
}

impl InternalClock {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn start(&mut self, tempo: u32, swing: u32) {
        self.phase = 0;
        self.swing_step = 11;
        self.set_tempo(tempo);
        self.set_swing(swing);
    }

    pub fn set_tempo(&mut self, tempo: u32) {
        // For 48kHz.
        self.phase_increment = 178_957u32.wrapping_mul(tempo) / 10;
    }

    pub fn set_swing(&mut self, swing: u32) {
        self.swing_amount = swing.wrapping_mul(0x8000_0000 / 3 / 100);
    }

    pub fn process(&mut self) -> bool {
        let mut half_cycle: u32 = 0x8000_0000;
        if self.swing_step < 6 {
            half_cycle = half_cycle.wrapping_add(self.swing_amount);
        } else {
            half_cycle = half_cycle.wrapping_sub(self.swing_amount);
        }

        let mut tick = false;
        if self.phase >= half_cycle {
            tick = true;
            self.phase -= half_cycle;
            self.swing_step += 1;
            if self.swing_step >= 12 {
                self.swing_step = 0;
            }
        }
        self.phase = self.phase.wrapping_add(self.phase_increment);
        tick
    }
}

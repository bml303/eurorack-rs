//! `edges/timer_oscillator.{h,cc}` -- channels 0..3: square waves the firmware
//! generates with the XMEGA's dual-slope PWM peripheral.
//!
//! The C only *computes* the timer registers (`period_`, `value_`, the
//! prescaler) and pokes them at the hardware. This port keeps that computation
//! verbatim and adds [`TimerOscillator::render_square`] -- a plain software
//! square generator driven by those registers -- so a host can actually hear
//! the channel. `render_square` is **not** part of the firmware.

use crate::resources::LUT_RES_TIMER_COUNT;

/// XMEGA clock: `F_CPU` in `edges/makefile`.
const F_CPU: f32 = 32_000_000.0;

const K_OCTAVE: i16 = 12 << 7;
const K_FIRST_NOTE_NORMAL_MODE: i16 = 24 << 7;
const K_FIRST_NOTE_LFO_MODE: i16 = -(12 << 7);

/// `pulse_widths[]` -- the compare value as a fraction of 256 (128 = 50 %).
const PULSE_WIDTHS: [u8; 5] = [128, 85, 64, 32, 13];

/// `PulseWidth`.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PulseWidth {
    Pw50 = 0,
    Pw66 = 1,
    Pw75 = 2,
    Pw87 = 3,
    Pw95 = 4,
    CvControlled = 5,
}

/// The timer clock prescalers Edges uses (`TimerPrescaler` in `avrlibx`, only
/// the three divisors that appear here).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TimerPrescaler {
    /// `TIMER_PRESCALER_CLK` -- divide by 1.
    Clk1,
    /// `TIMER_PRESCALER_CLK_8`.
    Clk8,
    /// `TIMER_PRESCALER_CLK_64` -- the "LFO mode" for very low pitches.
    Clk64,
}

impl TimerPrescaler {
    #[inline]
    fn divisor(self) -> f32 {
        match self {
            TimerPrescaler::Clk1 => 1.0,
            TimerPrescaler::Clk8 => 8.0,
            TimerPrescaler::Clk64 => 64.0,
        }
    }
}

#[inline]
fn u16_u8_mul_shift8(a: u16, b: u8) -> u16 {
    ((a as u32 * b as u32) >> 8) as u16
}

/// `edges::TimerOscillator`.
#[derive(Debug, Clone)]
pub struct TimerOscillator {
    value: u16,
    period: u16,
    cv_pw: u8,
    prescaler: TimerPrescaler,
    sq_phase: f32,
}

impl Default for TimerOscillator {
    fn default() -> Self {
        Self::new()
    }
}

impl TimerOscillator {
    pub fn new() -> Self {
        Self {
            value: 0,
            period: 0,
            cv_pw: 0,
            // `Init` sets the prescaler to CLK_8 and the timer to dual-slope PWM.
            prescaler: TimerPrescaler::Clk8,
            sq_phase: 0.0,
        }
    }

    /// `Init`.
    pub fn init(&mut self) {
        *self = Self::new();
    }

    /// `set_cv_pw(pw)` -- pulse width when [`PulseWidth::CvControlled`], clamped
    /// to `6..=250` as in the C.
    #[inline]
    pub fn set_cv_pw(&mut self, pw: u8) {
        self.cv_pw = pw.clamp(6, 250);
    }

    #[inline]
    pub fn period(&self) -> u16 {
        self.period
    }
    #[inline]
    pub fn value(&self) -> u16 {
        self.value
    }
    #[inline]
    pub fn prescaler(&self) -> TimerPrescaler {
        self.prescaler
    }

    /// `UpdatePitch(pitch, pulse_width)` -- `pitch` is `midi_note << 7`.
    /// The prescaler has hysteresis: it drops to `Clk64` ("LFO mode") below
    /// note 24 and only returns to `Clk8` above note 104.
    pub fn update_pitch(&mut self, pitch: i16, pulse_width: PulseWidth) {
        if pitch < (24 << 7) && self.prescaler != TimerPrescaler::Clk64 {
            self.prescaler = TimerPrescaler::Clk64;
        }
        if pitch > (104 << 7) && self.prescaler != TimerPrescaler::Clk8 {
            self.prescaler = TimerPrescaler::Clk8;
        }
        self.update_timer_parameters(pitch, pulse_width);
    }

    /// `SubFollow(t)` -- become a `/16` (one octave + `/2`-after-triangle)
    /// sub-oscillator of `t`, for the NES-triangle expander.
    pub fn sub_follow(&mut self, t: &TimerOscillator) {
        let period = t.period >> 1;
        self.value = period >> 1;
        self.period = period;
        self.prescaler = if t.prescaler == TimerPrescaler::Clk64 {
            TimerPrescaler::Clk8
        } else {
            TimerPrescaler::Clk1
        };
    }

    fn update_timer_parameters(&mut self, pitch: i16, pulse_width: PulseWidth) {
        let mut shifts: i32 = 0;

        let mut pitch = pitch.wrapping_sub(if self.prescaler == TimerPrescaler::Clk64 {
            K_FIRST_NOTE_LFO_MODE
        } else {
            K_FIRST_NOTE_NORMAL_MODE
        });

        while pitch < 0 {
            pitch = pitch.wrapping_add(K_OCTAVE);
        }
        while pitch >= K_OCTAVE {
            pitch -= K_OCTAVE;
            shifts += 1;
        }

        let index_integral = ((pitch as u16) >> 4) as usize;
        let index_fractional = ((pitch & 0xf) as u16 * 16) as u8;
        let count = LUT_RES_TIMER_COUNT[index_integral];
        let next = LUT_RES_TIMER_COUNT[index_integral + 1];
        let count = count.wrapping_sub(u16_u8_mul_shift8(
            count.wrapping_sub(next),
            index_fractional,
        ));

        self.period = count.checked_shr(shifts as u32).unwrap_or(0);
        let pw = if pulse_width == PulseWidth::CvControlled {
            self.cv_pw
        } else {
            PULSE_WIDTHS[pulse_width as usize]
        };
        self.value = u16_u8_mul_shift8(self.period, pw);
    }

    /// Software square-wave renderer (port-only -- the firmware uses hardware
    /// PWM). Output is 12-bit: `4095` while the counter is above `value`,
    /// `0` otherwise. `sample_rate` is the host's audio rate in Hz.
    pub fn render_square(&mut self, out: &mut [u16], sample_rate: f32, gate: bool) {
        if !gate || self.period == 0 {
            out.fill(0);
            return;
        }
        // Dual-slope PWM: one wave cycle is `2 * period` prescaled ticks.
        let cycle_ticks = 2.0 * self.period as f32 * self.prescaler.divisor();
        let freq = F_CPU / cycle_ticks;
        let phase_inc = freq / sample_rate;
        let duty = (self.value as f32 / self.period as f32).clamp(0.0, 1.0);

        let mut phase = self.sq_phase;
        for o in out.iter_mut() {
            *o = if phase < duty { 4095 } else { 0 };
            phase += phase_inc;
            if phase >= 1.0 {
                phase -= 1.0;
            }
        }
        self.sq_phase = phase;
    }
}

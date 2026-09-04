//! `plaits/dsp/oscillator/super_square_oscillator.h` -- two hard-sync'ed
//! square waves with a single meta-parameter, faking PWM. A hard-coded,
//! sync-always-on specialisation of [`super::variable_shape_oscillator`].

use stmlib::parameter_interpolator::ParameterInterpolator;
use stmlib::polyblep::{next_blep_sample, this_blep_sample};

use super::oscillator::MAX_FREQUENCY;

#[derive(Debug, Clone, Copy, Default)]
pub struct SuperSquareOscillator {
    master_phase: f32,
    slave_phase: f32,
    next_sample: f32,
    high: bool,
    master_frequency: f32,
    slave_frequency: f32,
}

impl SuperSquareOscillator {
    pub fn init(&mut self) {
        self.master_phase = 0.0;
        self.slave_phase = 0.0;
        self.next_sample = 0.0;
        self.high = false;
        self.master_frequency = 0.0;
        self.slave_frequency = 0.01;
    }

    pub fn render(&mut self, frequency: f32, shape: f32, out: &mut [f32]) {
        let mut master_frequency = frequency;
        let mut frequency = frequency
            * if shape < 0.5 {
                0.51 + 0.98 * shape
            } else {
                1.0 + 16.0 * (shape - 0.5) * (shape - 0.5)
            };

        if master_frequency >= MAX_FREQUENCY {
            master_frequency = MAX_FREQUENCY;
        }
        if frequency >= MAX_FREQUENCY {
            frequency = MAX_FREQUENCY;
        }

        let size = out.len();
        let mut master_fm = ParameterInterpolator::new(&mut self.master_frequency, master_frequency, size);
        let mut fm = ParameterInterpolator::new(&mut self.slave_frequency, frequency, size);

        let mut next_sample = self.next_sample;

        for o in out.iter_mut() {
            let mut reset = false;
            let mut transition_during_reset = false;
            let mut reset_time = 0.0f32;

            let mut this_sample = next_sample;
            next_sample = 0.0;

            let master_frequency = master_fm.next();
            let slave_frequency = fm.next();

            self.master_phase += master_frequency;
            if self.master_phase >= 1.0 {
                self.master_phase -= 1.0;
                reset_time = self.master_phase / master_frequency;

                let mut slave_phase_at_reset = self.slave_phase + (1.0 - reset_time) * slave_frequency;
                reset = true;
                if slave_phase_at_reset >= 1.0 {
                    slave_phase_at_reset -= 1.0;
                    transition_during_reset = true;
                }
                if !self.high && slave_phase_at_reset >= 0.5 {
                    transition_during_reset = true;
                }
                let value = if slave_phase_at_reset < 0.5 { 0.0 } else { 1.0 };
                this_sample -= value * this_blep_sample(reset_time);
                next_sample -= value * next_blep_sample(reset_time);
            }

            self.slave_phase += slave_frequency;
            while transition_during_reset || !reset {
                if !self.high {
                    if self.slave_phase < 0.5 {
                        break;
                    }
                    let t = (self.slave_phase - 0.5) / slave_frequency;
                    this_sample += this_blep_sample(t);
                    next_sample += next_blep_sample(t);
                    self.high = true;
                }

                if self.high {
                    if self.slave_phase < 1.0 {
                        break;
                    }
                    self.slave_phase -= 1.0;
                    let t = self.slave_phase / slave_frequency;
                    this_sample -= this_blep_sample(t);
                    next_sample -= next_blep_sample(t);
                    self.high = false;
                }
            }

            if reset {
                self.slave_phase = reset_time * slave_frequency;
                self.high = false;
            }

            next_sample += if self.slave_phase < 0.5 { 0.0 } else { 1.0 };
            *o = 2.0 * this_sample - 1.0;
        }

        self.next_sample = next_sample;
    }
}

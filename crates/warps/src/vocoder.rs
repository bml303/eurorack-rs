//! `warps/dsp/vocoder.{h,cc}` -- runs the modulator and carrier signals
//! through parallel [`FilterBank`]s, tracks a per-band envelope follower on
//! the modulator, and cross-fades the carrier bands between their own
//! level and the modulator's envelope (formant-shifted by walking the
//! envelope-follower peaks at a non-integer band offset).

use stmlib::units::semitones_to_ratio;

use crate::filter_bank::{FilterBank, MAX_FILTER_BANK_BLOCK_SIZE, NUM_BANDS};
use crate::limiter::Limiter;

/// `sqrtf(kNumBands)`, `kNumBands == 20`.
const K_FOLLOWER_GAIN: f32 = 4.472_135_955_f32;

#[derive(Debug, Clone, Copy, Default)]
pub struct EnvelopeFollower {
    attack: f32,
    decay: f32,
    envelope: f32,
    peak: f32,
    freeze: bool,
}

impl EnvelopeFollower {
    pub fn init(&mut self) {
        self.envelope = 0.0;
        self.freeze = false;
        self.attack = 0.1;
        self.decay = 0.1;
        self.peak = 0.0;
    }

    pub fn set_attack(&mut self, attack: f32) {
        self.attack = attack;
    }
    pub fn set_decay(&mut self, decay: f32) {
        self.decay = decay;
    }
    pub fn set_freeze(&mut self, freeze: bool) {
        self.freeze = freeze;
    }

    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let mut envelope = self.envelope;
        let attack = if self.freeze { 0.0 } else { self.attack };
        let decay = if self.freeze { 0.0 } else { self.decay };
        let mut peak = 0.0f32;
        for (i, o) in input.iter().zip(out.iter_mut()) {
            let error = (*i * K_FOLLOWER_GAIN).abs() - envelope;
            envelope += (if error > 0.0 { attack } else { decay }) * error;
            if envelope > peak {
                peak = envelope;
            }
            *o = envelope;
        }
        self.envelope = envelope;
        let error = peak - self.peak;
        self.peak += (if error > 0.0 { 0.5 } else { 0.1 }) * error;
    }

    pub fn peak(&self) -> f32 {
        self.peak
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct BandGain {
    pub carrier: f32,
    pub vocoder: f32,
}

pub struct Vocoder {
    release_time: f32,
    formant_shift: f32,

    previous_gain: [BandGain; NUM_BANDS],
    gain: [BandGain; NUM_BANDS],

    tmp: [f32; MAX_FILTER_BANK_BLOCK_SIZE],

    modulator_filter_bank: FilterBank,
    carrier_filter_bank: FilterBank,
    limiter: Limiter,
    follower: [EnvelopeFollower; NUM_BANDS],
}

impl Default for Vocoder {
    fn default() -> Self {
        Self {
            release_time: 0.5,
            formant_shift: 0.5,
            previous_gain: [BandGain::default(); NUM_BANDS],
            gain: [BandGain::default(); NUM_BANDS],
            tmp: [0.0; MAX_FILTER_BANK_BLOCK_SIZE],
            modulator_filter_bank: FilterBank::default(),
            carrier_filter_bank: FilterBank::default(),
            limiter: Limiter::default(),
            follower: [EnvelopeFollower::default(); NUM_BANDS],
        }
    }
}

impl Vocoder {
    pub fn init(&mut self, sample_rate: f32) {
        self.modulator_filter_bank.init(sample_rate);
        self.carrier_filter_bank.init(sample_rate);
        self.limiter.init();

        self.release_time = 0.5;
        self.formant_shift = 0.5;

        self.previous_gain = [BandGain::default(); NUM_BANDS];
        self.gain = [BandGain::default(); NUM_BANDS];

        for f in self.follower.iter_mut() {
            f.init();
        }
    }

    pub fn set_release_time(&mut self, release_time: f32) {
        self.release_time = release_time;
    }
    pub fn set_formant_shift(&mut self, formant_shift: f32) {
        self.formant_shift = formant_shift;
    }

    pub fn process(&mut self, modulator: &[f32], carrier: &[f32], out: &mut [f32], size: usize) {
        // Run through filter banks.
        self.modulator_filter_bank.analyze(modulator, size);
        self.carrier_filter_bank.analyze(carrier, size);

        // Set the attack/release release_time of envelope followers.
        let mut f = 80.0 * semitones_to_ratio(-72.0 * self.release_time);
        for i in 0..NUM_BANDS {
            let decay = f / self.modulator_filter_bank.band(i).sample_rate;
            self.follower[i].set_attack(decay * 2.0);
            self.follower[i].set_decay(decay * 0.5);
            self.follower[i].set_freeze(self.release_time > 0.995);
            f *= 1.2599; // 2 ** (4/12.0), a third octave.
        }

        // Compute the amplitude (or modulation amount) in all bands.
        let mut formant_shift_amount = 2.0 * (self.formant_shift - 0.5).abs();
        formant_shift_amount *= 2.0 - formant_shift_amount;
        formant_shift_amount *= 2.0 - formant_shift_amount;
        let envelope_increment = 4.0 * semitones_to_ratio(-48.0 * self.formant_shift);
        let mut envelope = 0.0f32;
        let k_last_band = NUM_BANDS as f32 - 1.0001;
        for i in 0..NUM_BANDS {
            let source_band = envelope.clamp(0.0, k_last_band);
            let source_band_integral = source_band as i32;
            let source_band_fractional = source_band - source_band_integral as f32;
            let a = self.follower[source_band_integral as usize].peak();
            let b = self.follower[source_band_integral as usize + 1].peak();
            let mut band_gain = a + (b - a) * source_band_fractional;
            let attenuation = envelope - k_last_band;
            if attenuation >= 0.0 {
                band_gain *= 1.0 / (1.0 + attenuation);
            }
            envelope += envelope_increment;

            self.gain[i].carrier = band_gain * formant_shift_amount;
            self.gain[i].vocoder = 1.0 - formant_shift_amount;
        }

        for i in 0..NUM_BANDS {
            let band_size = size / self.modulator_filter_bank.band(i).decimation_factor as usize;
            let step = 1.0 / band_size as f32;

            self.follower[i].process(&self.modulator_filter_bank.band(i).samples()[..band_size], &mut self.tmp[..band_size]);

            let mut vocoder_gain = self.previous_gain[i].vocoder;
            let vocoder_gain_increment = (self.gain[i].vocoder - vocoder_gain) * step;
            let mut carrier_gain = self.previous_gain[i].carrier;
            let carrier_gain_increment = (self.gain[i].carrier - carrier_gain) * step;

            let carrier = self.carrier_filter_bank.band_mut(i).samples_mut();
            for (c, &envelope) in carrier[..band_size].iter_mut().zip(self.tmp[..band_size].iter()) {
                *c *= carrier_gain + vocoder_gain * envelope;
                vocoder_gain += vocoder_gain_increment;
                carrier_gain += carrier_gain_increment;
            }

            self.previous_gain[i] = self.gain[i];
        }

        self.carrier_filter_bank.synthesize(out, size);
        self.limiter.process(&mut out[..size], 1.4);
    }
}

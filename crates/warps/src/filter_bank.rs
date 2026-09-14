//! `warps/dsp/filter_bank.{h,cc}` -- a 20-band vocoder analysis/synthesis
//! filter bank: 13 bands at fs/12, 6 at fs/3, 1 at full rate, each a
//! 2-stage `CrossoverSvf` band-pass (low-pass for band 0, high-pass for the
//! last band) plus a per-band group-delay-compensation delay line.
//!
//! Deviations from the C++: `Band::samples`/`delay_line` point into two
//! shared, hand-packed arenas (`samples_[kSampleMemorySize]`,
//! `delay_buffer_[kDelayLineSize]`) in the C++, purely to save RAM on the
//! STM32F3 -- each `Band` here just owns its own fixed-capacity buffers
//! instead (`[f32; MAX_FILTER_BANK_BLOCK_SIZE]`, a `PooledDelayLine` with a
//! `[f32; MAX_DELAY_LINE_SIZE]`), the same "drop the packed-arena memory
//! trick, it doesn't matter on a host" call already made for e.g. `mi-grids`'
//! `Options` union.

use stmlib::filter::{CrossoverSvf, FilterMode};

use crate::resources::FILTER_BANK_TABLE;
use crate::sample_rate_converter::{
    SampleRateConverterDown, SampleRateConverterUp, SRC_DOWN_3_36, SRC_DOWN_4_48, SRC_UP_3_36, SRC_UP_4_48,
};

pub const NUM_BANDS: usize = 20;
pub const LOW_FACTOR: i32 = 4;
pub const MID_FACTOR: i32 = 3;
pub const MAX_FILTER_BANK_BLOCK_SIZE: usize = 96;
/// `compensation / decimation_factor` is bounded by `max_delay <= 256`.
const MAX_DELAY_LINE_SIZE: usize = 257;

#[derive(Debug, Clone, Copy)]
pub struct PooledDelayLine {
    buffer: [f32; MAX_DELAY_LINE_SIZE],
    size: usize,
    head: usize,
}

impl Default for PooledDelayLine {
    fn default() -> Self {
        Self { buffer: [0.0; MAX_DELAY_LINE_SIZE], size: 1, head: 0 }
    }
}

impl PooledDelayLine {
    pub fn init(&mut self, delay: i32) {
        self.size = (delay + 1) as usize;
        self.head = 0;
        self.buffer = [0.0; MAX_DELAY_LINE_SIZE];
    }

    #[inline]
    pub fn read_write(&mut self, value: f32) -> f32 {
        self.buffer[self.head] = value;
        self.head = (self.head + 1) % self.size;
        self.buffer[self.head]
    }
}

#[derive(Debug, Clone, Copy)]
pub struct Band {
    pub group: i32,
    pub sample_rate: f32,
    pub post_gain: f32,
    svf: [CrossoverSvf; 2],
    pub decimation_factor: i32,
    samples: [f32; MAX_FILTER_BANK_BLOCK_SIZE],
    delay_line: PooledDelayLine,
    pub delay: i32,
}

impl Default for Band {
    fn default() -> Self {
        Self {
            group: 0,
            sample_rate: 0.0,
            post_gain: 0.0,
            svf: [CrossoverSvf::default(); 2],
            decimation_factor: 1,
            samples: [0.0; MAX_FILTER_BANK_BLOCK_SIZE],
            delay_line: PooledDelayLine::default(),
            delay: 0,
        }
    }
}

impl Band {
    #[inline]
    pub fn samples(&self) -> &[f32; MAX_FILTER_BANK_BLOCK_SIZE] {
        &self.samples
    }
    #[inline]
    pub fn samples_mut(&mut self) -> &mut [f32; MAX_FILTER_BANK_BLOCK_SIZE] {
        &mut self.samples
    }
}

pub struct FilterBank {
    mid_src_down: SampleRateConverterDown<36>,
    mid_src_up: SampleRateConverterUp<12>,
    low_src_down: SampleRateConverterDown<48>,
    low_src_up: SampleRateConverterUp<12>,

    tmp: [[f32; MAX_FILTER_BANK_BLOCK_SIZE]; 2],

    band: [Band; NUM_BANDS + 1],
}

impl Default for FilterBank {
    fn default() -> Self {
        Self {
            mid_src_down: SampleRateConverterDown::new(MID_FACTOR as usize, &SRC_DOWN_3_36),
            mid_src_up: SampleRateConverterUp::new(MID_FACTOR as usize, &SRC_UP_3_36),
            low_src_down: SampleRateConverterDown::new(LOW_FACTOR as usize, &SRC_DOWN_4_48),
            low_src_up: SampleRateConverterUp::new(LOW_FACTOR as usize, &SRC_UP_4_48),
            tmp: [[0.0; MAX_FILTER_BANK_BLOCK_SIZE]; 2],
            band: [Band::default(); NUM_BANDS + 1],
        }
    }
}

impl FilterBank {
    pub fn init(&mut self, sample_rate: f32) {
        self.low_src_down.init();
        self.low_src_up.init();
        self.mid_src_down.init();
        self.mid_src_up.init();

        let mut max_delay = 0i32;
        let mut group = -1i32;
        let mut decimation_factor = -1i32;
        // `i` indexes both `FILTER_BANK_TABLE` and `self.band` together.
        #[allow(clippy::needless_range_loop)]
        for i in 0..NUM_BANDS {
            let coefficients = FILTER_BANK_TABLE[i];

            let b = &mut self.band[i];
            b.decimation_factor = coefficients[0] as i32;

            if b.decimation_factor != decimation_factor {
                decimation_factor = b.decimation_factor;
                group += 1;
            }

            b.group = group;
            b.sample_rate = sample_rate / b.decimation_factor as f32;

            b.delay = coefficients[1] as i32;
            b.delay *= b.decimation_factor;
            b.post_gain = coefficients[2];

            max_delay = max_delay.max(b.delay);
            for pass in 0..2 {
                b.svf[pass].init();
                b.svf[pass].set_f_fq(coefficients[pass * 2 + 3], coefficients[pass * 2 + 4]);
            }
        }
        self.band[NUM_BANDS].group = self.band[NUM_BANDS - 1].group + 1;
        max_delay = max_delay.min(256);
        for i in 0..NUM_BANDS {
            let decimation_factor = self.band[i].decimation_factor;
            let group = self.band[i].group;
            let mut compensation = max_delay - self.band[i].delay;
            if group == 0 {
                compensation -= LOW_FACTOR * (self.low_src_down.delay() + self.low_src_up.delay());
                compensation -= self.mid_src_down.delay();
                compensation -= self.mid_src_up.delay();
            } else if group == 1 {
                compensation -= self.mid_src_down.delay();
                compensation -= self.mid_src_up.delay();
            }
            compensation = (compensation - decimation_factor / 2).max(0);
            self.band[i].delay_line.init(compensation / decimation_factor);
        }
    }

    pub fn analyze(&mut self, input: &[f32], size: usize) {
        self.mid_src_down.process(&input[..size], &mut self.tmp[0][..size / MID_FACTOR as usize]);
        let mid_size = size / MID_FACTOR as usize;
        let (tmp0, tmp1) = self.tmp.split_at_mut(1);
        self.low_src_down.process(&tmp0[0][..mid_size], &mut tmp1[0][..mid_size / LOW_FACTOR as usize]);

        for i in 0..NUM_BANDS {
            let group = self.band[i].group;
            let decimation_factor = self.band[i].decimation_factor;
            let band_size = size / decimation_factor as usize;

            // `sources[3] = { tmp_[1], tmp_[0], in }`.
            let source: &[f32] = match group {
                0 => &self.tmp[1][..band_size],
                1 => &self.tmp[0][..band_size],
                _ => &input[..band_size],
            };

            let mode = if i == 0 {
                FilterMode::LowPass
            } else if i == NUM_BANDS - 1 {
                FilterMode::HighPass
            } else {
                FilterMode::BandPassNormalized
            };

            let b = &mut self.band[i];
            b.svf[0].process(mode, source, &mut b.samples[..band_size]);
            b.svf[1].process_in_place(mode, &mut b.samples[..band_size]);

            let gain = b.post_gain;
            for s in b.samples[..band_size].iter_mut() {
                *s *= gain;
            }
        }
    }

    pub fn synthesize(&mut self, out: &mut [f32], size: usize) {
        let band0_size = size / self.band[0].decimation_factor as usize;
        self.tmp[1][..band0_size].fill(0.0);

        for i in 0..NUM_BANDS {
            let group = self.band[i].group;
            let decimation_factor = self.band[i].decimation_factor;
            let band_size = size / decimation_factor as usize;

            {
                let b = &mut self.band[i];
                let dest: &mut [f32] = match group {
                    0 => &mut self.tmp[1][..band_size],
                    1 => &mut self.tmp[0][..band_size],
                    _ => &mut out[..band_size],
                };
                for (d, &s) in dest.iter_mut().zip(b.samples[..band_size].iter()) {
                    *d += b.delay_line.read_write(s);
                }
            }

            if self.band[i + 1].group != group {
                if group == 0 {
                    let (tmp0, tmp1) = self.tmp.split_at_mut(1);
                    self.low_src_up.process(&tmp1[0][..band_size], &mut tmp0[0][..band_size * LOW_FACTOR as usize]);
                } else if group == 1 {
                    self.mid_src_up.process(&self.tmp[0][..band_size], &mut out[..band_size * MID_FACTOR as usize]);
                }
            }
        }
    }

    pub fn band(&self, index: usize) -> &Band {
        &self.band[index]
    }
    pub fn band_mut(&mut self, index: usize) -> &mut Band {
        &mut self.band[index]
    }
}

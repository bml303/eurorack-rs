//! `warps/dsp/sample_rate_converter.h` + `sample_rate_conversion_filters.h`
//! -- polyphase FIR interpolator (`SampleRateConverterUp`) / decimator
//! (`SampleRateConverterDown`) pair, used both for `Modulator`'s x6
//! oversampling around the cross-modulation algorithms and for
//! `FilterBank`'s x3/x4 band-splitting resampling.
//!
//! The C++ builds these at compile time from a recursive `FilterState<N>`
//! shift-register struct and an `Accumulator`/`PolyphaseStage` template
//! metaprogram that unrolls the polyphase decomposition -- effectively a
//! compile-time-specialised direct-form FIR. Only 3 `(ratio, filter_size)`
//! pairs are ever instantiated in the firmware: `(6, 48)` (`Modulator`'s
//! oversampling), `(4, 48)` and `(3, 36)` (`FilterBank`'s low/mid bands).
//! Rather than reproduce the template recursion, this port resolves the
//! polyphase index arithmetic once (worked out below) into a plain runtime
//! loop parameterised by a const-generic tap count, with `ratio` and the
//! half-length coefficient table (below) as ordinary fields -- mathematically
//! identical, just not unrolled at compile time.
//!
//! All 6 FIR responses are symmetric (linear phase), so each is stored as
//! just its first half (`h[j]` for `j < filter_size/2`); the back half is
//! `h[filter_size-1-j]`. `SampleRateConverterUp<TAPS>` needs `TAPS =
//! filter_size/ratio` (8, 12, 12 for the 3 pairs above) history samples;
//! `SampleRateConverterDown<FULL>` needs `FULL = filter_size` (48, 48, 36).

// Generated with `scipy.signal.remez(48, [0, 0.060000/6, 0.5/6, 0.5], [1, 0])`.
pub const SRC_UP_6_48: [f32; 24] = [
    4.357278576e-04, -2.297029461e-03, -4.703810602e-03, -8.774604727e-03,
    -1.433899145e-02, -2.112793398e-02, -2.853108802e-02, -3.552868193e-02,
    -4.069862931e-02, -4.228981313e-02, -3.836519645e-02, -2.700780696e-02,
    -6.569014106e-03, 2.407089704e-02, 6.526452513e-02, 1.164165703e-01,
    1.758932961e-01, 2.410483237e-01, 3.083744498e-01, 3.737697127e-01,
    4.328923682e-01, 4.815728403e-01, 5.162355916e-01, 5.342582974e-01,
];

// Generated with `scipy.signal.remez(48, [0, 0.060000/6, 0.5/6, 0.5], [1, 0])`.
pub const SRC_DOWN_6_48: [f32; 24] = [
    7.262130960e-05, -3.828382434e-04, -7.839684337e-04, -1.462434121e-03,
    -2.389831909e-03, -3.521322331e-03, -4.755181337e-03, -5.921446989e-03,
    -6.783104885e-03, -7.048302188e-03, -6.394199409e-03, -4.501301159e-03,
    -1.094835684e-03, 4.011816173e-03, 1.087742085e-02, 1.940276171e-02,
    2.931554935e-02, 4.017472062e-02, 5.139574163e-02, 6.229495212e-02,
    7.214872804e-02, 8.026214006e-02, 8.603926526e-02, 8.904304957e-02,
];

// Generated with `4 * scipy.signal.remez(48, [0, 0.105000/4, 0.5/4, 0.5], [1, 0])`.
pub const SRC_UP_4_48: [f32; 24] = [
    -6.014371929e-04, -1.116027480e-03, -1.547569918e-03, -1.288608084e-03,
    2.786886230e-04, 3.529342828e-03, 8.203156385e-03, 1.308970614e-02,
    1.600199910e-02, 1.419074690e-02, 5.231038872e-03, -1.177915684e-02,
    -3.506738553e-02, -5.953252182e-02, -7.699933415e-02, -7.757902368e-02,
    -5.198496872e-02, 5.703716839e-03, 9.559598586e-02, 2.106660616e-01,
    3.371310483e-01, 4.566603688e-01, 5.500087786e-01, 6.012053946e-01,
];

// Generated with `scipy.signal.remez(48, [0, 0.105000/4, 0.5/4, 0.5], [1, 0])`.
pub const SRC_DOWN_4_48: [f32; 24] = [
    -1.503592982e-04, -2.790068700e-04, -3.868924795e-04, -3.221520211e-04,
    6.967215575e-05, 8.823357070e-04, 2.050789096e-03, 3.272426536e-03,
    4.000499774e-03, 3.547686724e-03, 1.307759718e-03, -2.944789209e-03,
    -8.766846381e-03, -1.488313045e-02, -1.924983354e-02, -1.939475592e-02,
    -1.299624218e-02, 1.425929210e-03, 2.389899647e-02, 5.266651541e-02,
    8.428276207e-02, 1.141650922e-01, 1.375021946e-01, 1.503013486e-01,
];

// Generated with `3 * scipy.signal.remez(36, [0, 0.050000/3, 0.5/3, 0.5], [1, 0])`.
pub const SRC_UP_3_36: [f32; 18] = [
    2.111177486e-04, 9.399136027e-04, 2.516356933e-03, 4.847507152e-03,
    6.912087023e-03, 6.524576194e-03, 8.579855461e-04, -1.203466052e-02,
    -3.103696515e-02, -5.013495031e-02, -5.827142630e-02, -4.183809689e-02,
    1.038391226e-02, 1.014554664e-01, 2.222529437e-01, 3.515426263e-01,
    4.610075226e-01, 5.238640837e-01,
];

// Generated with `scipy.signal.remez(36, [0, 0.050000/3, 0.5/3, 0.5], [1, 0])`.
pub const SRC_DOWN_3_36: [f32; 18] = [
    7.037258286e-05, 3.133045342e-04, 8.387856444e-04, 1.615835717e-03,
    2.304029008e-03, 2.174858731e-03, 2.859951820e-04, -4.011553507e-03,
    -1.034565505e-02, -1.671165010e-02, -1.942380877e-02, -1.394603230e-02,
    3.461304086e-03, 3.381848881e-02, 7.408431457e-02, 1.171808754e-01,
    1.536691742e-01, 1.746213612e-01,
];

/// Upsampler: 1 input sample in, `ratio` output samples out.
#[derive(Debug, Clone, Copy)]
pub struct SampleRateConverterUp<const TAPS: usize> {
    ratio: usize,
    half_taps: &'static [f32],
    x: [f32; TAPS],
}

impl<const TAPS: usize> SampleRateConverterUp<TAPS> {
    pub fn new(ratio: usize, half_taps: &'static [f32]) -> Self {
        Self { ratio, half_taps, x: [0.0; TAPS] }
    }

    pub fn init(&mut self) {
        self.x = [0.0; TAPS];
    }

    pub fn delay(&self) -> i32 {
        (TAPS / 2) as i32
    }

    #[inline]
    fn full_tap(&self, j: usize) -> f32 {
        let half = self.half_taps.len();
        if j < half {
            self.half_taps[j]
        } else {
            self.half_taps[self.ratio * TAPS - 1 - j]
        }
    }

    /// `input.len()` input samples in, `input.len() * ratio` output samples
    /// (appended to `out`, phase-major: `out[n * ratio + p]`).
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        let ratio = self.ratio;
        for (n, &sample) in input.iter().enumerate() {
            for i in (1..TAPS).rev() {
                self.x[i] = self.x[i - 1];
            }
            self.x[0] = sample;
            for p in 0..ratio {
                let mut acc = 0.0f32;
                for (i, &x) in self.x.iter().enumerate() {
                    acc += x * self.full_tap(p + i * ratio);
                }
                out[n * ratio + p] = acc;
            }
        }
    }
}

/// Downsampler: `ratio` input samples in, 1 output sample out.
#[derive(Debug, Clone)]
pub struct SampleRateConverterDown<const FULL: usize> {
    ratio: usize,
    half_taps: &'static [f32],
    x: [f32; FULL],
    head: usize,
}

impl<const FULL: usize> SampleRateConverterDown<FULL> {
    pub fn new(ratio: usize, half_taps: &'static [f32]) -> Self {
        Self { ratio, half_taps, x: [0.0; FULL], head: 0 }
    }

    pub fn init(&mut self) {
        self.x = [0.0; FULL];
        self.head = 0;
    }

    pub fn delay(&self) -> i32 {
        (FULL / 2) as i32
    }

    #[inline]
    fn full_tap(&self, j: usize) -> f32 {
        let half = self.half_taps.len();
        if j < half {
            self.half_taps[j]
        } else {
            self.half_taps[FULL - 1 - j]
        }
    }

    #[inline]
    fn push(&mut self, sample: f32) {
        self.head = if self.head == 0 { FULL - 1 } else { self.head - 1 };
        self.x[self.head] = sample;
    }

    /// `input.len()` input samples in, `input.len() / ratio` output samples
    /// out. `input.len()` must be a multiple of `ratio`; like the C++, a
    /// mismatched length is a silent no-op.
    pub fn process(&mut self, input: &[f32], out: &mut [f32]) {
        if !input.len().is_multiple_of(self.ratio) {
            return;
        }
        for (chunk, o) in input.chunks(self.ratio).zip(out.iter_mut()) {
            for &sample in chunk {
                self.push(sample);
            }
            let mut acc = 0.0f32;
            for i in 0..FULL {
                acc += self.x[(self.head + i) % FULL] * self.full_tap(i);
            }
            *o = acc;
        }
    }
}

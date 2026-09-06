//! `clouds/dsp/parameters.h` -- the full control-parameter block passed to
//! every playback mode.

/// `Parameters` -- one flat struct fed to the processor each block. The
/// `granular` / `spectral` sub-structs hold the "meta" values that
/// `GranularProcessor::process_granular` derives from the user-facing knobs
/// before calling into a player.
#[derive(Debug, Clone, Copy, Default)]
pub struct Parameters {
    pub position: f32,
    pub size: f32,
    pub pitch: f32,
    pub density: f32,
    pub texture: f32,
    pub dry_wet: f32,
    pub stereo_spread: f32,
    pub feedback: f32,
    pub reverb: f32,

    pub freeze: bool,
    pub trigger: bool,
    pub gate: bool,

    pub granular: Granular,
    pub spectral: Spectral,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Granular {
    pub overlap: f32,
    pub window_shape: f32,
    pub stereo_spread: f32,
    pub use_deterministic_seed: bool,
}

#[derive(Debug, Clone, Copy, Default)]
pub struct Spectral {
    pub quantization: f32,
    pub refresh_rate: f32,
    pub phase_randomization: f32,
    pub warp: f32,
}

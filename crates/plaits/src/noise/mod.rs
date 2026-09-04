//! `plaits/dsp/noise/*`. `fractal_random_generator.h` isn't ported: it's
//! unused by every engine in the C source (dead code there too).

pub mod clocked_noise;
pub mod dust;
pub mod particle;
pub mod smooth_random_generator;

pub use clocked_noise::ClockedNoise;
pub use dust::dust;
pub use particle::Particle;
pub use smooth_random_generator::SmoothRandomGenerator;

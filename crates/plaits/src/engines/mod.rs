//! `plaits/dsp/engine/*` + `plaits/dsp/engine2/*` -- the 24 synthesis models.

pub mod additive_engine;
pub mod bass_drum_engine;
pub mod chord_engine;
pub mod fm_engine;
pub mod grain_engine;
pub mod hi_hat_engine;
pub mod modal_engine;
pub mod noise_engine;
pub mod particle_engine;
pub mod snare_drum_engine;
pub mod string_engine;
pub mod swarm_engine;
pub mod virtual_analog_engine;
pub mod wavetable_engine;
pub mod waveshaping_engine;

pub mod arpeggiator;
pub mod chiptune_engine;
pub mod phase_distortion_engine;
pub mod six_op_engine;
pub mod speech_engine;
pub mod string_machine_engine;
pub mod virtual_analog_vcf_engine;
pub mod wave_terrain_engine;

pub use additive_engine::AdditiveEngine;
pub use bass_drum_engine::BassDrumEngine;
pub use chord_engine::ChordEngine;
pub use fm_engine::FmEngine;
pub use grain_engine::GrainEngine;
pub use hi_hat_engine::HiHatEngine;
pub use modal_engine::ModalEngine;
pub use noise_engine::NoiseEngine;
pub use particle_engine::ParticleEngine;
pub use snare_drum_engine::SnareDrumEngine;
pub use string_engine::StringEngine;
pub use swarm_engine::SwarmEngine;
pub use virtual_analog_engine::VirtualAnalogEngine;
pub use wavetable_engine::WavetableEngine;
pub use waveshaping_engine::WaveshapingEngine;

pub use chiptune_engine::ChiptuneEngine;
pub use phase_distortion_engine::PhaseDistortionEngine;
pub use six_op_engine::SixOpEngine;
pub use speech_engine::SpeechEngine;
pub use string_machine_engine::StringMachineEngine;
pub use virtual_analog_vcf_engine::VirtualAnalogVcfEngine;
pub use wave_terrain_engine::WaveTerrainEngine;

//! `plaits/dsp/fm/patch.h` -- a DX7 voice patch, unpacked from its 128-byte
//! SysEx bulk-dump encoding (7-bit MIDI bytes, so each dumped byte only ever
//! carries a value the encoder already validated -- `unpack` still applies
//! the same defensive `min()` clamps as the C, in case of a corrupt dump).

pub const SYX_SIZE: usize = 128;

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Envelope {
    pub rate: [u8; 4],
    pub level: [u8; 4],
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct KeyboardScaling {
    pub break_point: u8,
    pub left_depth: u8,
    pub right_depth: u8,
    pub left_curve: u8,
    pub right_curve: u8,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct Operator {
    pub envelope: Envelope,
    pub keyboard_scaling: KeyboardScaling,

    pub rate_scaling: u8,
    pub amp_mod_sensitivity: u8,
    pub velocity_sensitivity: u8,
    pub level: u8,

    pub mode: u8,
    pub coarse: u8,
    /// Multiplies frequency by `1 + 0.01 * fine` (`mode == 0` only).
    pub fine: u8,
    pub detune: u8,
}

#[derive(Debug, Default, Clone, Copy, PartialEq, Eq)]
pub struct ModulationParameters {
    pub rate: u8,
    pub delay: u8,
    pub pitch_mod_depth: u8,
    pub amp_mod_depth: u8,
    pub reset_phase: u8,
    pub waveform: u8,
    pub pitch_mod_sensitivity: u8,
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct Patch {
    pub op: [Operator; 6],
    pub pitch_envelope: Envelope,

    pub algorithm: u8,
    pub feedback: u8,
    pub reset_phase: bool,

    pub modulations: ModulationParameters,

    pub transpose: u8,
    pub name: [u8; 10],
    pub active_operators: u8,
}

impl Patch {
    pub fn new() -> Self {
        Self::default()
    }

    /// Unpacks the 156-byte SysEx voice-dump layout (`data[0..17]` per
    /// operator x 6, then the shared pitch envelope/algorithm/LFO/name
    /// fields) starting at `data[0]`.
    pub fn unpack(&mut self, data: &[u8]) {
        for (i, op) in self.op.iter_mut().enumerate() {
            let op_data = &data[i * 17..];

            for j in 0..4 {
                op.envelope.rate[j] = (op_data[j] & 0x7f).min(99);
                op.envelope.level[j] = (op_data[4 + j] & 0x7f).min(99);
            }
            op.keyboard_scaling.break_point = (op_data[8] & 0x7f).min(99);
            op.keyboard_scaling.left_depth = (op_data[9] & 0x7f).min(99);
            op.keyboard_scaling.right_depth = (op_data[10] & 0x7f).min(99);
            op.keyboard_scaling.left_curve = op_data[11] & 0x3;
            op.keyboard_scaling.right_curve = (op_data[11] >> 2) & 0x3;

            op.rate_scaling = op_data[12] & 0x7;
            op.amp_mod_sensitivity = op_data[13] & 0x3;
            op.velocity_sensitivity = (op_data[13] >> 2) & 0x7;
            op.level = (op_data[14] & 0x7f).min(99);
            op.mode = op_data[15] & 0x1;
            op.coarse = (op_data[15] >> 1) & 0x1f;
            op.fine = (op_data[16] & 0x7f).min(99);
            op.detune = ((op_data[12] >> 3) & 0xf).min(14);
        }

        for j in 0..4 {
            self.pitch_envelope.rate[j] = (data[102 + j] & 0x7f).min(99);
            self.pitch_envelope.level[j] = (data[106 + j] & 0x7f).min(99);
        }

        self.algorithm = data[110] & 0x1f;
        self.feedback = data[111] & 0x7;
        self.reset_phase = (data[111] >> 3) & 0x1 != 0;

        self.modulations.rate = (data[112] & 0x7f).min(99);
        self.modulations.delay = (data[113] & 0x7f).min(99);
        self.modulations.pitch_mod_depth = (data[114] & 0x7f).min(99);
        self.modulations.amp_mod_depth = (data[115] & 0x7f).min(99);
        self.modulations.reset_phase = data[116] & 0x1;
        self.modulations.waveform = ((data[116] >> 1) & 0x7).min(5);
        self.modulations.pitch_mod_sensitivity = data[116] >> 4;

        self.transpose = (data[117] & 0x7f).min(48);

        for (i, c) in self.name.iter_mut().enumerate() {
            *c = data[118 + i] & 0x7f;
        }

        self.active_operators = 0x3f;
    }
}

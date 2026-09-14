//! `streams/meta_parameters.h` -- deriving several sub-parameters from one
//! knob position.

/// `ComputeAmountOffset` -- returns `(amount, offset)`.
pub fn compute_amount_offset(value: i32) -> (i32, i32) {
    if value < 32768 {
        let v = 32767 - value;
        let v = v.wrapping_mul(v) >> 15;
        ((32767 - v) << 1, 0)
    } else {
        (65535 - ((value - 32768) << 1), (value - 32768) << 1)
    }
}

/// `ComputeAttackDecay` -- returns `(attack, decay)`.
pub fn compute_attack_decay(shape: i32) -> (u16, u16) {
    if shape < 32768 {
        (0, (13 * (shape >> 3) + 12288) as u16)
    } else if shape < 49152 {
        let a = (shape - 32768) << 1;
        let d = 65535 - ((shape - 32768) >> 1) * 3;
        (a as u16, d as u16)
    } else {
        let a = 32768 - ((shape - 49152) >> 2) * 5;
        let d = 65535 - ((shape - 32768) >> 1) * 3;
        (a as u16, d as u16)
    }
}

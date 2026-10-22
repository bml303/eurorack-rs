//! `MacroOscillator` is built by value: a host builds it on its stack
//! before it reaches its home. The digital models' delay lines are on the
//! heap so it stays small enough for that -- inline they made it 40 KB.

use core::mem::size_of;
use braids::MacroOscillator;

#[test]
fn the_oscillator_is_small_enough_to_build_on_a_stack() {
    let size = size_of::<MacroOscillator>();
    assert!(size <= 4 * 1024, "MacroOscillator is {size} bytes; a new inline buffer?");
}

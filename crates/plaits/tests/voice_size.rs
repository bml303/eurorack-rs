//! `Voice` is built by value: a host without placement-new builds it on its
//! stack (twice over, with the struct that wraps it) before it reaches its
//! home. The engines' large buffers are on the heap so the voice stays
//! small enough for that -- an inline 32 KB diffuser ring and 15 KB of
//! string delay lines once made it 72 KB, 130 KB of stack at a host's first
//! render.

use core::mem::size_of;
use plaits::Voice;

#[test]
fn the_voice_is_small_enough_to_build_on_a_stack() {
    let size = size_of::<Voice>();
    assert!(size <= 16 * 1024, "Voice is {size} bytes; a new inline buffer in an engine?");
}

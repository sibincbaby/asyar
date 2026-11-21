//! Shared keyboard input simulation logic.

use enigo::{Enigo, Key, KeyboardControllable};

/// Simulates a paste chord (Ctrl+V).
pub fn post_paste_chord() {
    post_key_chord_via_enigo(Key::Control, 'v');
}

/// Simulates a copy chord (Ctrl+C).
pub fn post_copy_chord_to_frontmost() {
    post_key_chord_via_enigo(Key::Control, 'c');
}

/// Shared helper for enigo-based modifier chords.
pub fn post_key_chord_via_enigo(modifier: Key, key_char: char) {
    let mut enigo = Enigo::new();
    enigo.key_down(modifier);
    enigo.key_click(Key::Layout(key_char));
    enigo.key_up(modifier);
}

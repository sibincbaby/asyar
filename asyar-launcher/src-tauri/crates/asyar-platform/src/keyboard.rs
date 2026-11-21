//! Cross-platform keyboard layout resolution abstractions.

pub trait KeyboardLayoutResolver {
    fn resolve_keypress(&self, key: &rdev::Key, shift_held: bool) -> Option<char>;
}

/// System-default keyboard layout resolver dispatching to the active platform's OS APIs.
#[derive(Debug, Clone, Copy, Default)]
pub struct SystemKeyboardResolver;

impl KeyboardLayoutResolver for SystemKeyboardResolver {
    #[cfg(target_os = "linux")]
    fn resolve_keypress(&self, key: &rdev::Key, shift_held: bool) -> Option<char> {
        crate::linux_key_resolver::resolve_keypress(*key, shift_held)
    }
}

/// Resolves a keypress using the system's active keyboard layout.
pub fn resolve_keypress(key: &rdev::Key, shift_held: bool) -> Option<char> {
    SystemKeyboardResolver.resolve_keypress(key, shift_held)
}

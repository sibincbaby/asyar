//! The global hotkey on X11, grabbed by the daemon itself like the Tauri app
//! does, with one addition: after the hotkey fires, the keyboard stays grabbed
//! until its modifiers are released.
//!
//! Without that, a quick Alt release reaches the app behind before the
//! launcher has focus. Electron, Chrome and Firefox read Alt down, Alt up with
//! nothing between as a lone Alt tap and open their menu, which then holds the
//! keyboard so the next hotkey press goes nowhere. Cinnamon does the same with
//! its start menu for a lone Super.

use std::time::{Duration, Instant};

use x11rb::CURRENT_TIME;
use x11rb::connection::Connection;
use x11rb::protocol::Event;
use x11rb::protocol::xproto::{ConnectionExt, GrabMode, GrabStatus, Keycode, ModMask};
use x11rb::rust_connection::RustConnection;

const SHIFT: u16 = 1;
const LOCK: u16 = 1 << 1;
const CONTROL: u16 = 1 << 2;
const MOD1: u16 = 1 << 3;
const MOD2: u16 = 1 << 4; // NumLock
const MOD4: u16 = 1 << 6;
/// Modifiers that make a different shortcut; lock keys are ignored.
const RELEVANT: u16 = SHIFT | CONTROL | MOD1 | MOD4;
/// Never hold the keyboard longer than this, even if a release is missed.
const MAX_HOLD: Duration = Duration::from_secs(3);

/// Parses `Alt+Space`, `Ctrl+Shift+K`, `Super+F2`… into modifier bits and a
/// keysym. Keys: a letter or digit, `Space`, `Enter`, `Tab`, `F1`–`F24`.
pub fn parse(spec: &str) -> Result<(u16, u32), String> {
    let mut mods = 0;
    let mut key = None;
    for part in spec.split('+').map(str::trim) {
        match part.to_ascii_lowercase().as_str() {
            "ctrl" | "control" => mods |= CONTROL,
            "alt" => mods |= MOD1,
            "shift" => mods |= SHIFT,
            "super" | "meta" | "win" => mods |= MOD4,
            name => {
                let sym = keysym(name).ok_or_else(|| format!("unknown key {part:?}"))?;
                if key.replace(sym).is_some() {
                    return Err("more than one key".into());
                }
            }
        }
    }
    match key {
        Some(_) if mods == 0 => Err("needs a modifier".into()),
        Some(key) => Ok((mods, key)),
        None => Err("no key".into()),
    }
}

fn keysym(name: &str) -> Option<u32> {
    match name {
        "space" => Some(0x20),
        "enter" | "return" => Some(0xff0d),
        "tab" => Some(0xff09),
        _ => {
            if let Some(n) = name.strip_prefix('f').and_then(|n| n.parse::<u32>().ok()) {
                return (1..=24).contains(&n).then_some(0xffbe + n - 1);
            }
            let mut chars = name.chars();
            match (chars.next(), chars.next()) {
                // Lowercase ASCII letters and digits are their own keysyms.
                (Some(c), None) if c.is_ascii_alphanumeric() => Some(c as u32),
                _ => None,
            }
        }
    }
}

/// Grabs `spec` on its own X connection and calls `on_press` from a
/// background thread each time it is pressed.
pub fn start(spec: &str, on_press: impl Fn() + Send + 'static) -> Result<(), String> {
    let (mods, sym) = parse(spec)?;
    let (conn, screen) = x11rb::connect(None).map_err(|e| e.to_string())?;
    let root = conn.setup().roots[screen].root;
    let keycode = keycode_for(&conn, sym)?.ok_or("key is not on this keyboard")?;
    let held_keys = modifier_keycodes(&conn, mods)?;
    for locks in [0, LOCK, MOD2, LOCK | MOD2] {
        conn.grab_key(
            false,
            root,
            ModMask::from(mods | locks),
            keycode,
            GrabMode::ASYNC,
            GrabMode::ASYNC,
        )
        .map_err(|e| e.to_string())?
        .check()
        // BadAccess: another app or a desktop shortcut holds the key.
        .map_err(|e| format!("already taken? {e}"))?;
    }
    std::thread::spawn(move || {
        if let Err(error) = listen(&conn, root, keycode, mods, &held_keys, on_press) {
            eprintln!("[asyar-gpui] hotkey stopped: {error}");
        }
    });
    Ok(())
}

fn listen(
    conn: &RustConnection,
    root: u32,
    keycode: Keycode,
    mods: u16,
    held_keys: &[Keycode],
    on_press: impl Fn(),
) -> anyhow::Result<()> {
    let mut holding: Option<Instant> = None;
    loop {
        let Some(since) = holding else {
            if let Event::KeyPress(press) = conn.wait_for_event()?
                && press.detail == keycode
                && u16::from(press.state) & RELEVANT == mods
            {
                on_press();
                // Same client as the passive grab, so this extends it past
                // the key's release instead of failing as already grabbed.
                let grabbed = conn
                    .grab_keyboard(false, root, press.time, GrabMode::ASYNC, GrabMode::ASYNC)?
                    .reply()?
                    .status
                    == GrabStatus::SUCCESS;
                holding = grabbed.then(Instant::now);
            }
            continue;
        };
        // While holding, events (including autorepeat) are swallowed; poll so
        // the grab is released as soon as the modifiers are up.
        while conn.poll_for_event()?.is_some() {}
        if !any_down(conn, held_keys)? || since.elapsed() > MAX_HOLD {
            conn.ungrab_keyboard(CURRENT_TIME)?;
            conn.flush()?;
            holding = None;
        } else {
            std::thread::sleep(Duration::from_millis(5));
        }
    }
}

fn keycode_for(conn: &RustConnection, sym: u32) -> Result<Option<Keycode>, String> {
    let (min, max) = (conn.setup().min_keycode, conn.setup().max_keycode);
    let map = conn
        .get_keyboard_mapping(min, max - min + 1)
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    let per = usize::from(map.keysyms_per_keycode).max(1);
    Ok(map
        .keysyms
        .chunks(per)
        .position(|syms| syms.contains(&sym))
        .and_then(|ix| u8::try_from(ix).ok())
        .map(|ix| min + ix))
}

/// The physical keys behind `mods` (Alt_L and Alt_R for Mod1, …).
fn modifier_keycodes(conn: &RustConnection, mods: u16) -> Result<Vec<Keycode>, String> {
    let map = conn
        .get_modifier_mapping()
        .map_err(|e| e.to_string())?
        .reply()
        .map_err(|e| e.to_string())?;
    let per = usize::from(map.keycodes_per_modifier()).max(1);
    Ok(map
        .keycodes
        .chunks(per)
        .enumerate()
        .filter(|(bit, _)| mods & (1 << bit) != 0)
        .flat_map(|(_, codes)| codes.iter().copied().filter(|&code| code != 0))
        .collect())
}

fn any_down(conn: &RustConnection, keys: &[Keycode]) -> anyhow::Result<bool> {
    let keymap = conn.query_keymap()?.reply()?.keys;
    Ok(keys
        .iter()
        .any(|&code| keymap[usize::from(code / 8)] & (1 << (code % 8)) != 0))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_modifiers_and_keys() {
        assert_eq!(parse("Alt+Space"), Ok((MOD1, 0x20)));
        assert_eq!(parse("ctrl + shift + K"), Ok((CONTROL | SHIFT, 'k' as u32)));
        assert_eq!(parse("Super+F2"), Ok((MOD4, 0xffbf)));
        assert_eq!(parse("Alt+F"), Ok((MOD1, 'f' as u32)));
    }

    #[test]
    fn rejects_bad_hotkeys() {
        assert!(parse("Space").is_err(), "needs a modifier");
        assert!(parse("Alt").is_err(), "needs a key");
        assert!(parse("Alt+A+B").is_err());
        assert!(parse("Alt+F25").is_err());
        assert!(parse("Alt+Banana").is_err());
    }
}

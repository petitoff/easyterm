use winit::keyboard::{Key, ModifiersState, NamedKey};

pub(crate) fn named_key_bytes(key: &Key) -> Option<&'static [u8]> {
    match key {
        Key::Named(NamedKey::Enter) => Some(b"\r"),
        Key::Named(NamedKey::Tab) => Some(b"\t"),
        Key::Named(NamedKey::Backspace) => Some(b"\x7f"),
        Key::Named(NamedKey::Escape) => Some(b"\x1b"),
        Key::Named(NamedKey::ArrowUp) => Some(b"\x1b[A"),
        Key::Named(NamedKey::ArrowDown) => Some(b"\x1b[B"),
        Key::Named(NamedKey::ArrowRight) => Some(b"\x1b[C"),
        Key::Named(NamedKey::ArrowLeft) => Some(b"\x1b[D"),
        Key::Named(NamedKey::Home) => Some(b"\x1b[H"),
        Key::Named(NamedKey::End) => Some(b"\x1b[F"),
        Key::Named(NamedKey::Delete) => Some(b"\x1b[3~"),
        Key::Named(NamedKey::Insert) => Some(b"\x1b[2~"),
        Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~"),
        Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~"),
        _ => None,
    }
}

pub(crate) fn control_sequence_for_key(key: &Key) -> Option<Vec<u8>> {
    match key {
        Key::Character(value) if value.chars().count() == 1 => {
            let ch = value.chars().next()?.to_ascii_lowercase();
            if ch.is_ascii_lowercase() {
                Some(vec![(ch as u8) - b'a' + 1])
            } else {
                None
            }
        }
        _ => None,
    }
}

pub(crate) fn should_forward_text(text: &str, modifiers: ModifiersState) -> bool {
    if text.is_empty() {
        return false;
    }
    if modifiers.control_key() || modifiers.super_key() {
        return false;
    }
    !text
        .chars()
        .any(|ch| ch.is_control() && ch != '\r' && ch != '\n' && ch != '\t')
}

pub(crate) fn normalize_paste(text: &str) -> Vec<u8> {
    text.replace("\r\n", "\n").replace('\n', "\r").into_bytes()
}

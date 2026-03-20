use crate::gui::tab::CellPoint;
use easyterm_core::{MouseReportingMode, TerminalModes};
use winit::event::MouseButton;
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

pub(crate) fn named_key_bytes(key: &Key, modes: TerminalModes) -> Option<Vec<u8>> {
    match key {
        Key::Named(NamedKey::Enter) => Some(b"\r".to_vec()),
        Key::Named(NamedKey::Tab) => Some(b"\t".to_vec()),
        Key::Named(NamedKey::Backspace) => Some(b"\x7f".to_vec()),
        Key::Named(NamedKey::Escape) => Some(b"\x1b".to_vec()),
        Key::Named(NamedKey::ArrowUp) => Some(cursor_sequence('A', modes)),
        Key::Named(NamedKey::ArrowDown) => Some(cursor_sequence('B', modes)),
        Key::Named(NamedKey::ArrowRight) => Some(cursor_sequence('C', modes)),
        Key::Named(NamedKey::ArrowLeft) => Some(cursor_sequence('D', modes)),
        Key::Named(NamedKey::Home) => Some(if modes.application_cursor_keys {
            b"\x1bOH".to_vec()
        } else {
            b"\x1b[H".to_vec()
        }),
        Key::Named(NamedKey::End) => Some(if modes.application_cursor_keys {
            b"\x1bOF".to_vec()
        } else {
            b"\x1b[F".to_vec()
        }),
        Key::Named(NamedKey::Delete) => Some(b"\x1b[3~".to_vec()),
        Key::Named(NamedKey::Insert) => Some(b"\x1b[2~".to_vec()),
        Key::Named(NamedKey::PageUp) => Some(b"\x1b[5~".to_vec()),
        Key::Named(NamedKey::PageDown) => Some(b"\x1b[6~".to_vec()),
        Key::Named(NamedKey::F1) => Some(b"\x1bOP".to_vec()),
        Key::Named(NamedKey::F2) => Some(b"\x1bOQ".to_vec()),
        Key::Named(NamedKey::F3) => Some(b"\x1bOR".to_vec()),
        Key::Named(NamedKey::F4) => Some(b"\x1bOS".to_vec()),
        Key::Named(NamedKey::F5) => Some(b"\x1b[15~".to_vec()),
        Key::Named(NamedKey::F6) => Some(b"\x1b[17~".to_vec()),
        Key::Named(NamedKey::F7) => Some(b"\x1b[18~".to_vec()),
        Key::Named(NamedKey::F8) => Some(b"\x1b[19~".to_vec()),
        Key::Named(NamedKey::F9) => Some(b"\x1b[20~".to_vec()),
        Key::Named(NamedKey::F10) => Some(b"\x1b[21~".to_vec()),
        Key::Named(NamedKey::F11) => Some(b"\x1b[23~".to_vec()),
        Key::Named(NamedKey::F12) => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}

pub(crate) fn physical_key_bytes(key: &PhysicalKey, modes: TerminalModes) -> Option<Vec<u8>> {
    match key {
        PhysicalKey::Code(KeyCode::Enter) => Some(b"\r".to_vec()),
        PhysicalKey::Code(KeyCode::Tab) => Some(b"\t".to_vec()),
        PhysicalKey::Code(KeyCode::Backspace) => Some(b"\x7f".to_vec()),
        PhysicalKey::Code(KeyCode::Escape) => Some(b"\x1b".to_vec()),
        PhysicalKey::Code(KeyCode::ArrowUp) => Some(cursor_sequence('A', modes)),
        PhysicalKey::Code(KeyCode::ArrowDown) => Some(cursor_sequence('B', modes)),
        PhysicalKey::Code(KeyCode::ArrowRight) => Some(cursor_sequence('C', modes)),
        PhysicalKey::Code(KeyCode::ArrowLeft) => Some(cursor_sequence('D', modes)),
        PhysicalKey::Code(KeyCode::Home) => Some(if modes.application_cursor_keys {
            b"\x1bOH".to_vec()
        } else {
            b"\x1b[H".to_vec()
        }),
        PhysicalKey::Code(KeyCode::End) => Some(if modes.application_cursor_keys {
            b"\x1bOF".to_vec()
        } else {
            b"\x1b[F".to_vec()
        }),
        PhysicalKey::Code(KeyCode::Delete) => Some(b"\x1b[3~".to_vec()),
        PhysicalKey::Code(KeyCode::Insert) => Some(b"\x1b[2~".to_vec()),
        PhysicalKey::Code(KeyCode::PageUp) => Some(b"\x1b[5~".to_vec()),
        PhysicalKey::Code(KeyCode::PageDown) => Some(b"\x1b[6~".to_vec()),
        PhysicalKey::Code(KeyCode::F1) => Some(b"\x1bOP".to_vec()),
        PhysicalKey::Code(KeyCode::F2) => Some(b"\x1bOQ".to_vec()),
        PhysicalKey::Code(KeyCode::F3) => Some(b"\x1bOR".to_vec()),
        PhysicalKey::Code(KeyCode::F4) => Some(b"\x1bOS".to_vec()),
        PhysicalKey::Code(KeyCode::F5) => Some(b"\x1b[15~".to_vec()),
        PhysicalKey::Code(KeyCode::F6) => Some(b"\x1b[17~".to_vec()),
        PhysicalKey::Code(KeyCode::F7) => Some(b"\x1b[18~".to_vec()),
        PhysicalKey::Code(KeyCode::F8) => Some(b"\x1b[19~".to_vec()),
        PhysicalKey::Code(KeyCode::F9) => Some(b"\x1b[20~".to_vec()),
        PhysicalKey::Code(KeyCode::F10) => Some(b"\x1b[21~".to_vec()),
        PhysicalKey::Code(KeyCode::F11) => Some(b"\x1b[23~".to_vec()),
        PhysicalKey::Code(KeyCode::F12) => Some(b"\x1b[24~".to_vec()),
        _ => None,
    }
}

pub(crate) fn modified_key_bytes(
    key: &Key,
    physical_key: &PhysicalKey,
    modifiers: ModifiersState,
    modes: TerminalModes,
) -> Option<Vec<u8>> {
    if modifiers.shift_key() && !modifiers.control_key() && !modifiers.alt_key() {
        if let Key::Named(NamedKey::Tab) = key {
            return Some(b"\x1b[Z".to_vec());
        }
    }

    if modifiers.control_key() && !modifiers.alt_key() {
        return control_sequence_for_key(key);
    }

    if let Some(bytes) = physical_key_bytes(physical_key, modes) {
        return Some(bytes);
    }

    named_key_bytes(key, modes)
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

pub(crate) fn normalize_paste(text: &str, modes: TerminalModes) -> Vec<u8> {
    let normalized = text.replace("\r\n", "\n").replace('\n', "\r");
    if modes.bracketed_paste {
        let mut out = Vec::with_capacity(normalized.len() + 12);
        out.extend_from_slice(b"\x1b[200~");
        out.extend_from_slice(normalized.as_bytes());
        out.extend_from_slice(b"\x1b[201~");
        out
    } else {
        normalized.into_bytes()
    }
}

pub(crate) fn should_capture_mouse(modes: TerminalModes) -> bool {
    modes.mouse_reporting != MouseReportingMode::Off
}

pub(crate) fn encode_mouse_button(
    button: MouseButton,
    pressed: bool,
    point: CellPoint,
    modifiers: ModifiersState,
    modes: TerminalModes,
) -> Option<Vec<u8>> {
    let base = if pressed {
        base_button_code(button)?
    } else {
        3
    };
    Some(encode_mouse_event(
        base + modifier_bits(modifiers),
        point,
        pressed,
        modes,
    ))
}

pub(crate) fn encode_mouse_motion(
    pressed_button: Option<MouseButton>,
    point: CellPoint,
    modifiers: ModifiersState,
    modes: TerminalModes,
) -> Option<Vec<u8>> {
    let code = match modes.mouse_reporting {
        MouseReportingMode::Drag => base_button_code(pressed_button?)? + 32,
        MouseReportingMode::Motion => {
            let base = pressed_button.and_then(base_button_code).unwrap_or(3);
            base + 32
        }
        MouseReportingMode::Off | MouseReportingMode::Click => return None,
    };

    Some(encode_mouse_event(
        code + modifier_bits(modifiers),
        point,
        true,
        modes,
    ))
}

pub(crate) fn encode_mouse_wheel(
    delta_lines: i32,
    point: CellPoint,
    modifiers: ModifiersState,
    modes: TerminalModes,
) -> Vec<Vec<u8>> {
    let code = if delta_lines >= 0 { 64 } else { 65 } + modifier_bits(modifiers);
    let count = delta_lines.unsigned_abs().max(1) as usize;
    (0..count)
        .map(|_| encode_mouse_event(code, point, true, modes))
        .collect()
}

fn control_sequence_for_key(key: &Key) -> Option<Vec<u8>> {
    match key {
        Key::Character(value) if value.chars().count() == 1 => {
            let ch = value.chars().next()?;
            match ch {
                'a'..='z' | 'A'..='Z' => Some(vec![(ch.to_ascii_lowercase() as u8) - b'a' + 1]),
                ' ' | '@' | '2' => Some(vec![0x00]),
                '[' | '3' => Some(vec![0x1b]),
                '\\' | '4' => Some(vec![0x1c]),
                ']' | '5' => Some(vec![0x1d]),
                '^' | '6' => Some(vec![0x1e]),
                '_' | '7' | '/' => Some(vec![0x1f]),
                '8' | '?' => Some(vec![0x7f]),
                _ => None,
            }
        }
        _ => None,
    }
}

fn cursor_sequence(final_byte: char, modes: TerminalModes) -> Vec<u8> {
    if modes.application_cursor_keys {
        format!("\x1bO{final_byte}").into_bytes()
    } else {
        format!("\x1b[{final_byte}").into_bytes()
    }
}

fn base_button_code(button: MouseButton) -> Option<u8> {
    match button {
        MouseButton::Left => Some(0),
        MouseButton::Middle => Some(1),
        MouseButton::Right => Some(2),
        _ => None,
    }
}

fn modifier_bits(modifiers: ModifiersState) -> u8 {
    let mut bits = 0;
    if modifiers.shift_key() {
        bits += 4;
    }
    if modifiers.alt_key() {
        bits += 8;
    }
    if modifiers.control_key() {
        bits += 16;
    }
    bits
}

fn encode_mouse_event(code: u8, point: CellPoint, pressed: bool, modes: TerminalModes) -> Vec<u8> {
    let col = point.col.saturating_add(1);
    let row = point.global_row.saturating_add(1);

    if modes.sgr_mouse {
        format!(
            "\x1b[<{};{};{}{}",
            code,
            col,
            row,
            if pressed { 'M' } else { 'm' }
        )
        .into_bytes()
    } else {
        vec![
            0x1b,
            b'[',
            b'M',
            code.saturating_add(32),
            (col.min(223) as u8).saturating_add(32),
            (row.min(223) as u8).saturating_add(32),
        ]
    }
}

#[cfg(test)]
mod tests {
    use super::{encode_mouse_wheel, modified_key_bytes, normalize_paste, should_capture_mouse};
    use crate::gui::tab::CellPoint;
    use easyterm_core::{MouseReportingMode, TerminalModes};
    use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

    #[test]
    fn arrows_switch_to_application_cursor_mode() {
        let normal = modified_key_bytes(
            &Key::Named(NamedKey::ArrowUp),
            &PhysicalKey::Code(KeyCode::ArrowUp),
            ModifiersState::default(),
            TerminalModes::default(),
        )
        .unwrap();
        let application = modified_key_bytes(
            &Key::Named(NamedKey::ArrowUp),
            &PhysicalKey::Code(KeyCode::ArrowUp),
            ModifiersState::default(),
            TerminalModes {
                application_cursor_keys: true,
                ..TerminalModes::default()
            },
        )
        .unwrap();

        assert_eq!(normal, b"\x1b[A");
        assert_eq!(application, b"\x1bOA");
    }

    #[test]
    fn bracketed_paste_wraps_payload() {
        let bytes = normalize_paste(
            "hello\nworld",
            TerminalModes {
                bracketed_paste: true,
                ..TerminalModes::default()
            },
        );

        assert_eq!(bytes, b"\x1b[200~hello\rworld\x1b[201~");
    }

    #[test]
    fn mouse_reporting_captures_mouse() {
        assert!(!should_capture_mouse(TerminalModes {
            alternate_screen: true,
            ..TerminalModes::default()
        }));
        assert!(should_capture_mouse(TerminalModes {
            mouse_reporting: MouseReportingMode::Click,
            ..TerminalModes::default()
        }));
    }

    #[test]
    fn wheel_uses_sgr_mouse_when_enabled() {
        let packets = encode_mouse_wheel(
            1,
            CellPoint {
                global_row: 4,
                col: 2,
            },
            ModifiersState::default(),
            TerminalModes {
                sgr_mouse: true,
                ..TerminalModes::default()
            },
        );

        assert_eq!(packets, vec![b"\x1b[<64;3;5M".to_vec()]);
    }
}

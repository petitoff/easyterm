use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Color {
    Default,
    Indexed(u8),
    Rgb(u8, u8, u8),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct Style {
    pub fg: Color,
    pub bg: Color,
    pub bold: bool,
    pub dim: bool,
    pub italic: bool,
    pub underline: bool,
    pub inverse: bool,
    pub hidden: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            dim: false,
            italic: false,
            underline: false,
            inverse: false,
            hidden: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearMode {
    ToEnd,
    ToStart,
    All,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DecMode {
    ApplicationCursorKeys,
    MouseClickReporting,
    MouseDragReporting,
    MouseMotionReporting,
    SgrMouse,
    AlternateScreen,
    BracketedPaste,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnsiEvent {
    Print(char),
    NewLine,
    CarriageReturn,
    Backspace,
    Index,
    ReverseIndex,
    NextLine,
    SaveCursor,
    RestoreCursor,
    CursorUp(u16),
    CursorDown(u16),
    CursorForward(u16),
    CursorBackward(u16),
    CursorHorizontalAbsolute(u16),
    CursorPosition {
        row: u16,
        col: u16,
    },
    SetScrollRegion {
        top: Option<u16>,
        bottom: Option<u16>,
    },
    InsertBlankChars(u16),
    DeleteChars(u16),
    EraseChars(u16),
    InsertLines(u16),
    DeleteLines(u16),
    ClearLine(ClearMode),
    ClearScreen(ClearMode),
    SetStyle(Vec<u16>),
    SetWindowTitle(String),
    SetDecMode {
        mode: DecMode,
        enabled: bool,
    },
}

pub fn parse_ansi(input: &[u8]) -> Vec<AnsiEvent> {
    parse_ansi_stream(input).0
}

pub fn parse_ansi_stream(input: &[u8]) -> (Vec<AnsiEvent>, usize) {
    let mut events = Vec::new();
    let mut i = 0;

    while i < input.len() {
        match input[i] {
            b'\n' => {
                events.push(AnsiEvent::NewLine);
                i += 1;
            }
            b'\r' => {
                events.push(AnsiEvent::CarriageReturn);
                i += 1;
            }
            0x08 => {
                events.push(AnsiEvent::Backspace);
                i += 1;
            }
            0x1b => {
                if i + 1 >= input.len() {
                    break;
                }

                if input[i + 1] == b'[' {
                    let mut j = i + 2;
                    while j < input.len() {
                        let byte = input[j];
                        if (0x40..=0x7e).contains(&byte) {
                            break;
                        }
                        j += 1;
                    }

                    if j < input.len() {
                        let final_byte = input[j] as char;
                        let params = std::str::from_utf8(&input[i + 2..j]).unwrap_or("");
                        decode_csi(final_byte, params, &mut events);
                        i = j + 1;
                    } else {
                        break;
                    }
                } else if input[i + 1] == b']' {
                    if let Some((event, next_i)) = parse_osc_sequence(input, i + 2) {
                        if let Some(event) = event {
                            events.push(event);
                        }
                        i = next_i;
                    } else {
                        break;
                    }
                } else if let Some((event, next_i)) = parse_escape_sequence(input, i + 1) {
                    if let Some(event) = event {
                        events.push(event);
                    }
                    i = next_i;
                } else {
                    break;
                }
            }
            byte => {
                if let Some(ch) = decode_utf8_char(&input[i..]) {
                    let len = ch.len_utf8();
                    events.push(AnsiEvent::Print(ch));
                    i += len;
                } else if utf8_sequence_incomplete(&input[i..]) {
                    break;
                } else {
                    events.push(AnsiEvent::Print(byte as char));
                    i += 1;
                }
            }
        }
    }

    (events, i)
}

fn parse_numbers(params: &str) -> Vec<u16> {
    if params.is_empty() {
        return Vec::new();
    }

    params
        .split(';')
        .map(|part| part.parse::<u16>().unwrap_or(0))
        .collect()
}

fn decode_csi(final_byte: char, params: &str, events: &mut Vec<AnsiEvent>) {
    let private = params.strip_prefix('?');
    let numbers = parse_numbers(private.unwrap_or(params));

    match final_byte {
        'A' => events.push(AnsiEvent::CursorUp(first_or(&numbers, 1))),
        'B' => events.push(AnsiEvent::CursorDown(first_or(&numbers, 1))),
        'C' => events.push(AnsiEvent::CursorForward(first_or(&numbers, 1))),
        'D' => events.push(AnsiEvent::CursorBackward(first_or(&numbers, 1))),
        'G' => events.push(AnsiEvent::CursorHorizontalAbsolute(first_or(&numbers, 1))),
        'H' | 'f' => {
            let row = *numbers.first().unwrap_or(&1);
            let col = *numbers.get(1).unwrap_or(&1);
            events.push(AnsiEvent::CursorPosition { row, col });
        }
        '@' => events.push(AnsiEvent::InsertBlankChars(first_or(&numbers, 1))),
        'P' => events.push(AnsiEvent::DeleteChars(first_or(&numbers, 1))),
        'X' => events.push(AnsiEvent::EraseChars(first_or(&numbers, 1))),
        'L' => events.push(AnsiEvent::InsertLines(first_or(&numbers, 1))),
        'M' => events.push(AnsiEvent::DeleteLines(first_or(&numbers, 1))),
        'J' => events.push(AnsiEvent::ClearScreen(parse_clear_mode(first_or(
            &numbers, 0,
        )))),
        'K' => events.push(AnsiEvent::ClearLine(parse_clear_mode(first_or(
            &numbers, 0,
        )))),
        'r' => events.push(AnsiEvent::SetScrollRegion {
            top: numbers.first().copied(),
            bottom: numbers.get(1).copied(),
        }),
        'm' => {
            if numbers.is_empty() {
                events.push(AnsiEvent::SetStyle(vec![0]));
            } else {
                events.push(AnsiEvent::SetStyle(numbers));
            }
        }
        's' => events.push(AnsiEvent::SaveCursor),
        'u' if private.is_none() => events.push(AnsiEvent::RestoreCursor),
        'h' | 'l' if private.is_some() => {
            let enabled = final_byte == 'h';
            for number in numbers {
                if let Some(mode) = dec_mode(number) {
                    events.push(AnsiEvent::SetDecMode { mode, enabled });
                }
            }
        }
        _ => {}
    }
}

fn dec_mode(number: u16) -> Option<DecMode> {
    match number {
        1 => Some(DecMode::ApplicationCursorKeys),
        47 | 1047 | 1049 => Some(DecMode::AlternateScreen),
        1000 => Some(DecMode::MouseClickReporting),
        1002 => Some(DecMode::MouseDragReporting),
        1003 => Some(DecMode::MouseMotionReporting),
        1006 => Some(DecMode::SgrMouse),
        2004 => Some(DecMode::BracketedPaste),
        _ => None,
    }
}

fn first_or(numbers: &[u16], default: u16) -> u16 {
    *numbers.first().unwrap_or(&default)
}

fn parse_clear_mode(value: u16) -> ClearMode {
    match value {
        1 => ClearMode::ToStart,
        2 => ClearMode::All,
        _ => ClearMode::ToEnd,
    }
}

fn decode_utf8_char(bytes: &[u8]) -> Option<char> {
    for len in 1..=4 {
        if bytes.len() < len {
            break;
        }

        if let Ok(value) = std::str::from_utf8(&bytes[..len]) {
            if let Some(ch) = value.chars().next() {
                if ch.len_utf8() == len {
                    return Some(ch);
                }
            }
        }
    }

    None
}

fn utf8_sequence_incomplete(bytes: &[u8]) -> bool {
    let Some(&first) = bytes.first() else {
        return false;
    };

    let expected_len = match first {
        0x00..=0x7f => return false,
        0xc2..=0xdf => 2,
        0xe0..=0xef => 3,
        0xf0..=0xf4 => 4,
        _ => return false,
    };

    bytes.len() < expected_len
}

fn parse_osc_sequence(input: &[u8], start: usize) -> Option<(Option<AnsiEvent>, usize)> {
    let mut end = start;
    while end < input.len() {
        match input[end] {
            0x07 => break,
            0x1b if end + 1 < input.len() && input[end + 1] == b'\\' => break,
            _ => end += 1,
        }
    }

    if end >= input.len() {
        return None;
    }

    let next_i = if input[end] == 0x07 { end + 1 } else { end + 2 };
    let payload = std::str::from_utf8(&input[start..end]).ok()?;
    let mut parts = payload.splitn(2, ';');
    let code = parts.next().unwrap_or_default();
    let value = parts.next().unwrap_or_default();

    let event = match code {
        "0" | "2" if !value.is_empty() => Some(AnsiEvent::SetWindowTitle(value.to_string())),
        _ => None,
    };

    Some((event, next_i))
}

fn parse_escape_sequence(input: &[u8], start: usize) -> Option<(Option<AnsiEvent>, usize)> {
    let byte = *input.get(start)?;
    let next_i = start + 1;
    let event = match byte {
        b'D' => Some(AnsiEvent::Index),
        b'M' => Some(AnsiEvent::ReverseIndex),
        b'E' => Some(AnsiEvent::NextLine),
        b'7' => Some(AnsiEvent::SaveCursor),
        b'8' => Some(AnsiEvent::RestoreCursor),
        b'(' | b')' | b'*' | b'+' => {
            if input.get(start + 1).is_some() {
                return Some((None, start + 2));
            }
            return None;
        }
        b'=' | b'>' => None,
        _ => return Some((None, next_i)),
    };

    if matches!(byte, b'=' | b'>') {
        Some((None, next_i))
    } else {
        Some((event, next_i))
    }
}

#[cfg(test)]
mod tests {
    use super::{parse_ansi, parse_ansi_stream, AnsiEvent, ClearMode, Color, DecMode, Style};

    #[test]
    fn parses_text_and_control_flow() {
        let events = parse_ansi(b"ab\r\n");
        assert_eq!(
            events,
            vec![
                AnsiEvent::Print('a'),
                AnsiEvent::Print('b'),
                AnsiEvent::CarriageReturn,
                AnsiEvent::NewLine,
            ]
        );
    }

    #[test]
    fn parses_styles_and_cursor_moves() {
        let events = parse_ansi(b"\x1b[31;1mA\x1b[2D\x1b[2K");
        assert_eq!(
            events,
            vec![
                AnsiEvent::SetStyle(vec![31, 1]),
                AnsiEvent::Print('A'),
                AnsiEvent::CursorBackward(2),
                AnsiEvent::ClearLine(ClearMode::All),
            ]
        );
    }

    #[test]
    fn parses_private_modes() {
        let events = parse_ansi(b"\x1b[?1;1006;2004h\x1b[?1;2004l");
        assert_eq!(
            events,
            vec![
                AnsiEvent::SetDecMode {
                    mode: DecMode::ApplicationCursorKeys,
                    enabled: true,
                },
                AnsiEvent::SetDecMode {
                    mode: DecMode::SgrMouse,
                    enabled: true,
                },
                AnsiEvent::SetDecMode {
                    mode: DecMode::BracketedPaste,
                    enabled: true,
                },
                AnsiEvent::SetDecMode {
                    mode: DecMode::ApplicationCursorKeys,
                    enabled: false,
                },
                AnsiEvent::SetDecMode {
                    mode: DecMode::BracketedPaste,
                    enabled: false,
                },
            ]
        );
    }

    #[test]
    fn parses_escape_sequences_used_by_vim() {
        let events = parse_ansi(b"\x1b(B\x1b7\x1b8\x1bM\x1bD\x1bE");
        assert_eq!(
            events,
            vec![
                AnsiEvent::SaveCursor,
                AnsiEvent::RestoreCursor,
                AnsiEvent::ReverseIndex,
                AnsiEvent::Index,
                AnsiEvent::NextLine,
            ]
        );
    }

    #[test]
    fn parses_scroll_region_and_line_ops() {
        let events = parse_ansi(b"\x1b[7G\x1b[3@\x1b[4P\x1b[5X\x1b[s\x1b[u\x1b[2;5r\x1b[2L\x1b[3M");
        assert_eq!(
            events,
            vec![
                AnsiEvent::CursorHorizontalAbsolute(7),
                AnsiEvent::InsertBlankChars(3),
                AnsiEvent::DeleteChars(4),
                AnsiEvent::EraseChars(5),
                AnsiEvent::SaveCursor,
                AnsiEvent::RestoreCursor,
                AnsiEvent::SetScrollRegion {
                    top: Some(2),
                    bottom: Some(5),
                },
                AnsiEvent::InsertLines(2),
                AnsiEvent::DeleteLines(3),
            ]
        );
    }

    #[test]
    fn ignores_osc_sequences() {
        let events = parse_ansi(b"\x1b]0;easyterm\x07hello");
        assert_eq!(
            events,
            vec![
                AnsiEvent::SetWindowTitle("easyterm".into()),
                AnsiEvent::Print('h'),
                AnsiEvent::Print('e'),
                AnsiEvent::Print('l'),
                AnsiEvent::Print('l'),
                AnsiEvent::Print('o'),
            ]
        );
    }

    #[test]
    fn parses_osc_title_with_st_terminator() {
        let events = parse_ansi(b"\x1b]2;build:api\x1b\\");
        assert_eq!(events, vec![AnsiEvent::SetWindowTitle("build:api".into())]);
    }

    #[test]
    fn style_default_is_stable() {
        assert_eq!(
            Style::default(),
            Style {
                fg: Color::Default,
                bg: Color::Default,
                bold: false,
                dim: false,
                italic: false,
                underline: false,
                inverse: false,
                hidden: false,
            }
        );
    }

    #[test]
    fn preserves_incomplete_csi_for_next_chunk() {
        let (events, consumed) = parse_ansi_stream(b"\x1b[67");
        assert!(events.is_empty());
        assert_eq!(consumed, 0);
    }

    #[test]
    fn preserves_incomplete_utf8_for_next_chunk() {
        let (events, consumed) = parse_ansi_stream(&[0xe2, 0x94]);
        assert!(events.is_empty());
        assert_eq!(consumed, 0);
    }
}

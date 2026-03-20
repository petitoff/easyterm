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
    pub italic: bool,
    pub underline: bool,
}

impl Default for Style {
    fn default() -> Self {
        Self {
            fg: Color::Default,
            bg: Color::Default,
            bold: false,
            italic: false,
            underline: false,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ClearMode {
    ToEnd,
    ToStart,
    All,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AnsiEvent {
    Print(char),
    NewLine,
    CarriageReturn,
    Backspace,
    CursorUp(u16),
    CursorDown(u16),
    CursorForward(u16),
    CursorBackward(u16),
    CursorPosition { row: u16, col: u16 },
    ClearLine(ClearMode),
    ClearScreen(ClearMode),
    SetStyle(Vec<u16>),
}

pub fn parse_ansi(input: &[u8]) -> Vec<AnsiEvent> {
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
                if i + 1 < input.len() && input[i + 1] == b'[' {
                    let mut j = i + 2;
                    while j < input.len() {
                        let byte = input[j];
                        if (byte as char).is_ascii_alphabetic() {
                            break;
                        }
                        j += 1;
                    }

                    if j < input.len() {
                        let final_byte = input[j] as char;
                        let params = std::str::from_utf8(&input[i + 2..j]).unwrap_or("");
                        let numbers = parse_numbers(params);
                        decode_csi(final_byte, numbers, &mut events);
                        i = j + 1;
                    } else {
                        break;
                    }
                } else if i + 1 < input.len() && input[i + 1] == b']' {
                    i = skip_osc_sequence(input, i + 2);
                } else {
                    i += 1;
                }
            }
            byte => {
                if let Some(ch) = decode_utf8_char(&input[i..]) {
                    let len = ch.len_utf8();
                    events.push(AnsiEvent::Print(ch));
                    i += len;
                } else {
                    events.push(AnsiEvent::Print(byte as char));
                    i += 1;
                }
            }
        }
    }

    events
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

fn decode_csi(final_byte: char, numbers: Vec<u16>, events: &mut Vec<AnsiEvent>) {
    match final_byte {
        'A' => events.push(AnsiEvent::CursorUp(first_or(&numbers, 1))),
        'B' => events.push(AnsiEvent::CursorDown(first_or(&numbers, 1))),
        'C' => events.push(AnsiEvent::CursorForward(first_or(&numbers, 1))),
        'D' => events.push(AnsiEvent::CursorBackward(first_or(&numbers, 1))),
        'H' | 'f' => {
            let row = *numbers.first().unwrap_or(&1);
            let col = *numbers.get(1).unwrap_or(&1);
            events.push(AnsiEvent::CursorPosition { row, col });
        }
        'J' => events.push(AnsiEvent::ClearScreen(parse_clear_mode(first_or(
            &numbers, 0,
        )))),
        'K' => events.push(AnsiEvent::ClearLine(parse_clear_mode(first_or(
            &numbers, 0,
        )))),
        'm' => {
            if numbers.is_empty() {
                events.push(AnsiEvent::SetStyle(vec![0]));
            } else {
                events.push(AnsiEvent::SetStyle(numbers));
            }
        }
        _ => {}
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

fn skip_osc_sequence(input: &[u8], mut i: usize) -> usize {
    while i < input.len() {
        match input[i] {
            0x07 => return i + 1,
            0x1b if i + 1 < input.len() && input[i + 1] == b'\\' => return i + 2,
            _ => i += 1,
        }
    }
    input.len()
}

#[cfg(test)]
mod tests {
    use super::{parse_ansi, AnsiEvent, ClearMode, Color, Style};

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
    fn ignores_osc_sequences() {
        let events = parse_ansi(b"\x1b]0;easyterm\x07hello");
        assert_eq!(
            events,
            vec![
                AnsiEvent::Print('h'),
                AnsiEvent::Print('e'),
                AnsiEvent::Print('l'),
                AnsiEvent::Print('l'),
                AnsiEvent::Print('o'),
            ]
        );
    }

    #[test]
    fn style_default_is_stable() {
        assert_eq!(
            Style::default(),
            Style {
                fg: Color::Default,
                bg: Color::Default,
                bold: false,
                italic: false,
                underline: false,
            }
        );
    }
}

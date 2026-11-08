//! Cell-wise ASCII projection covers provider labels and overlay borders.
//! IDs and numeric slots stay intact; unsupported name glyphs are explicitly lossy.
use ratatui::Frame;
pub(super) fn finish(frame: &mut Frame) {
    for cell in &mut frame.buffer_mut().content {
        if cell.symbol().is_ascii() {
            continue;
        }
        let c = match cell.symbol().chars().next().unwrap_or(' ') as u32 {
            0x2500 | 0x2501 | 0x2550 => '-',
            0x2502 | 0x2503 | 0x2551 => '|',
            0x250c | 0x2510 | 0x2514 | 0x2518 | 0x256d | 0x256e | 0x256f | 0x2570 => '+',
            0x2022 | 0x00b7 | 0x2026 => '.',
            0x00e0..=0x00e5 => 'a',
            0x00c0..=0x00c5 => 'A',
            0x00e8..=0x00eb => 'e',
            0x00c8..=0x00cb => 'E',
            0x00ec..=0x00ef => 'i',
            0x00cc..=0x00cf => 'I',
            0x00f2..=0x00f6 | 0x00f8 => 'o',
            0x00d2..=0x00d6 | 0x00d8 => 'O',
            0x00f9..=0x00fc => 'u',
            0x00d9..=0x00dc => 'U',
            0x00e7 => 'c',
            0x00c7 => 'C',
            0x00f1 => 'n',
            0x00d1 => 'N',
            _ => '?',
        };
        cell.set_symbol(&c.to_string());
    }
}

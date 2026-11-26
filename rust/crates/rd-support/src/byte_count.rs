//! File sizes written like Foundation's `ByteCountFormatter` (`.file` style, English).

/// `ByteCountFormatter.string(fromByteCount:countStyle: .file)` for the en locale:
/// decimal units, `KB` without decimals, `MB` with one, larger units with two, half-up
/// rounding and trailing zeros dropped (`1 byte`, `999 bytes`, `2 KB`, `1.2 MB`, `1.23 GB`).
pub fn byte_count_file_style(bytes: u64) -> String {
    if bytes == 0 {
        return "Zero KB".to_owned();
    }
    if bytes == 1 {
        return "1 byte".to_owned();
    }
    if bytes < 1000 {
        return format!("{bytes} bytes");
    }
    const UNITS: [(&str, u32); 5] = [("KB", 0), ("MB", 1), ("GB", 2), ("TB", 2), ("PB", 2)];
    let value = u128::from(bytes);
    let mut index = 0;
    while index + 1 < UNITS.len() && value >= 1000u128.pow(index as u32 + 2) {
        index += 1;
    }
    loop {
        let (name, decimals) = UNITS[index];
        let unit = 1000u128.pow(index as u32 + 1);
        let scale = 10u128.pow(decimals);
        let scaled = (value * scale * 2 + unit) / (unit * 2);
        if scaled >= 1000 * scale && index + 1 < UNITS.len() {
            index += 1;
            continue;
        }
        let whole = scaled / scale;
        let fraction = scaled % scale;
        if fraction == 0 {
            return format!("{whole} {name}");
        }
        let digits = format!("{fraction:0width$}", width = decimals as usize);
        return format!("{whole}.{} {name}", digits.trim_end_matches('0'));
    }
}

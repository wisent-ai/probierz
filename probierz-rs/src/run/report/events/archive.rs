use crate::run::{fs, DeflateDecoder, Path, Read};
use crate::run::*;
pub(crate) fn zip_entries(file: &Path) -> Result<Vec<(String, String)>, String> {
    let buffer = fs::read(file).map_err(|error| error.to_string())?;
    if buffer.len() < 22 {
        return Err("zip end record missing".into());
    }
    let minimum = buffer.len().saturating_sub(65_557);
    let mut end = None;
    for offset in (minimum..=buffer.len() - 22).rev() {
        if read_u32(&buffer, offset) == Some(0x06054b50) {
            end = Some(offset);
            break;
        }
    }
    let end = end.ok_or("zip end record missing")?;
    let count = read_u16(&buffer, end + 10).ok_or("invalid zip end record")? as usize;
    let mut offset = read_u32(&buffer, end + 16).ok_or("invalid zip end record")? as usize;
    let mut entries = Vec::new();
    for _ in 0..count {
        if read_u32(&buffer, offset) != Some(0x02014b50) {
            return Err("invalid zip central directory".into());
        }
        let method = read_u16(&buffer, offset + 10).ok_or("invalid zip central directory")?;
        let size = read_u32(&buffer, offset + 20).ok_or("invalid zip central directory")? as usize;
        let name_len =
            read_u16(&buffer, offset + 28).ok_or("invalid zip central directory")? as usize;
        let extra_len =
            read_u16(&buffer, offset + 30).ok_or("invalid zip central directory")? as usize;
        let comment_len =
            read_u16(&buffer, offset + 32).ok_or("invalid zip central directory")? as usize;
        let local = read_u32(&buffer, offset + 42).ok_or("invalid zip central directory")? as usize;
        let name = String::from_utf8_lossy(
            buffer
                .get(offset + 46..offset + 46 + name_len)
                .ok_or("invalid zip central directory")?,
        )
        .into_owned();
        if read_u32(&buffer, local) != Some(0x04034b50) {
            return Err("invalid zip local header".into());
        }
        let local_name = read_u16(&buffer, local + 26).ok_or("invalid zip local header")? as usize;
        let local_extra = read_u16(&buffer, local + 28).ok_or("invalid zip local header")? as usize;
        let data_at = local + 30 + local_name + local_extra;
        let compressed = buffer
            .get(data_at..data_at + size)
            .ok_or("invalid zip data")?;
        let content = if method == 0 {
            Some(compressed.to_vec())
        } else if method == 8 {
            let mut decoded = Vec::new();
            DeflateDecoder::new(compressed)
                .read_to_end(&mut decoded)
                .map_err(|error| error.to_string())?;
            Some(decoded)
        } else {
            None
        };
        if let Some(content) = content {
            entries.push((name, String::from_utf8_lossy(&content).into_owned()));
        }
        offset += 46 + name_len + extra_len + comment_len;
    }
    Ok(entries)
}
pub(crate) fn read_u16(bytes: &[u8], at: usize) -> Option<u16> {
    Some(u16::from_le_bytes(bytes.get(at..at + 2)?.try_into().ok()?))
}
pub(crate) fn read_u32(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}
pub(crate) fn json_lines(content: &str) -> Vec<Value> {
    content
        .lines()
        .filter_map(|line| {
            (!line.trim().is_empty())
                .then(|| serde_json::from_str(line).ok())
                .flatten()
        })
        .collect()
}


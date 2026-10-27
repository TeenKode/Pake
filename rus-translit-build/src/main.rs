#![cfg_attr(windows, windows_subsystem = "windows")]
use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};

fn main() {
    if let Err(err) = run() {
        if let Ok(path) = std::env::current_exe() {
            if let Some(dir) = path.parent() {
                let _ = fs::write(dir.join("translit_error.log"), err.to_string());
            }
        }
    }
}

fn run() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "No executable directory"))?;
    let dest = dir.join("Готовые");
    fs::create_dir_all(&dest)?;
    let mut errors = Vec::new();
    for entry in fs::read_dir(dir)? {
        let entry = match entry { Ok(e) => e, Err(e) => { errors.push(e.to_string()); continue; } };
        let from = entry.path();
        if from == exe || !entry.file_type().map(|t| t.is_file()).unwrap_or(false) { continue; }
        let name = entry.file_name();
        let name = name.to_string_lossy();
        if !name.chars().any(is_russian) { continue; }
        let target_name = rename(&name);
        if let Err(e) = copy_unique(&from, &dest, &target_name) {
            errors.push(format!("{}: {}", name, e));
        }
    }
    if !errors.is_empty() {
        let _ = fs::write(dest.join("_translit_errors.log"), errors.join("\n"));
    }
    Ok(())
}

fn is_russian(c: char) -> bool {
    matches!(c, 'А'..='я' | 'Ё' | 'ё')
}

fn transliterate(part: &str) -> String {
    let mut result = String::new();
    for c in part.chars() {
        if c.is_whitespace() {
            if !result.ends_with('-') { result.push('-'); }
            continue;
        }
        let latin = match c {
            'а'=>"a", 'б'=>"b", 'в'=>"v", 'г'=>"g", 'д'=>"d", 'е'=>"e", 'ё'=>"yo", 'ж'=>"zh", 'з'=>"z",
            'и'=>"i", 'й'=>"y", 'к'=>"k", 'л'=>"l", 'м'=>"m", 'н'=>"n", 'о'=>"o", 'п'=>"p", 'р'=>"r",
            'с'=>"s", 'т'=>"t", 'у'=>"u", 'ф'=>"f", 'х'=>"kh", 'ц'=>"ts", 'ч'=>"ch", 'ш'=>"sh",
            'щ'=>"shch", 'ъ'=>"", 'ы'=>"y", 'ь'=>"", 'э'=>"e", 'ю'=>"yu", 'я'=>"ya",
            'А'=>"A", 'Б'=>"B", 'В'=>"V", 'Г'=>"G", 'Д'=>"D", 'Е'=>"E", 'Ё'=>"Yo", 'Ж'=>"Zh", 'З'=>"Z",
            'И'=>"I", 'Й'=>"Y", 'К'=>"K", 'Л'=>"L", 'М'=>"M", 'Н'=>"N", 'О'=>"O", 'П'=>"P", 'Р'=>"R",
            'С'=>"S", 'Т'=>"T", 'У'=>"U", 'Ф'=>"F", 'Х'=>"Kh", 'Ц'=>"Ts", 'Ч'=>"Ch", 'Ш'=>"Sh",
            'Щ'=>"Shch", 'Ъ'=>"", 'Ы'=>"Y", 'Ь'=>"", 'Э'=>"E", 'Ю'=>"Yu", 'Я'=>"Ya",
            _ => { result.push(c); continue; }
        };
        result.push_str(latin);
    }
    result
}
fn split_name(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (name, None)
    }
}
fn truncate_utf16(input: &str, max: usize) -> String {
    let mut out = String::new();
    let mut length = 0;
    for c in input.chars() {
        length += c.len_utf16();
        if length > max { break; }
        out.push(c);
    }
    out
}
fn reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") ||
       (base.len() == 4 && (base.starts_with("COM") || base.starts_with("LPT"))
           && matches!(base.as_bytes()[3], b'1'..=b'9'))
}
fn rename(name: &str) -> String {
    let (stem, ext) = split_name(name);
    let mut stem = transliterate(stem).trim_end_matches('.').to_string();
    if stem.is_empty() { stem = "file".to_string(); }
    if reserved(&stem) { stem.push('_'); }
    stem = truncate_utf16(&stem, 180);
    match ext.map(transliterate).filter(|s| !s.is_empty()) {
        Some(ext) => format!("{}.{}", stem, truncate_utf16(&ext, 50)),
        None => stem
    }
}
fn copy_unique(src: &Path, folder: &Path, proposed: &str) -> io::Result<PathBuf> {
    let (stem, ext) = split_name(proposed);
    for i in 1..=100_000usize {
        let name = if i == 1 { proposed.to_string() } else {
            let suffix = format!("_{}", i);
            let stem = truncate_utf16(stem, 180usize.saturating_sub(suffix.len()));
            match ext {
                Some(ext) => format!("{}{}.{}", stem, suffix, ext),
                None => format!("{}{}", stem, suffix)
            }
        };
        let output = folder.join(name);
        match OpenOptions::new().write(true).create_new(true).open(&output) {
            Ok(mut file) => {
                let result = (|| { let mut input = File::open(src)?; io::copy(&mut input, &mut file)?; file.flush() })();
                if let Err(error) = result {
                    drop(file);
                    let _ = fs::remove_file(&output);
                    return Err(error);
                }
                return Ok(output)
            }
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e)
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Too many name collisions"))
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn handles_russian_spaces_and_extensions() {
        assert_eq!(rename("Мой документ.pdf"), "Moy-dokument.pdf");
        assert_eq!(rename("Отчёт за 2026 год.docx"), "Otchyot-za-2026-god.docx");
        assert_eq!(rename("Ёжик   и щука.jpg"), "Yozhik-i-shchuka.jpg");
        assert_eq!(rename("Русское имя.рф"), "Russkoe-imya.rf");
        assert_eq!(rename("Тест-  пример.txt"), "Test-primer.txt");
    }
    #[test]
    fn keeps_original_and_handles_collisions() {
        let path = std::env::temp_dir().join(format!("rus_translit_test_{}", std::process::id()));
        let output = path.join("Готовые");
        fs::create_dir_all(&output).unwrap();
        let source = path.join("Тест.txt");
        fs::write(&source, b"HELLO").unwrap();
        assert!(copy_unique(&source, &output, "Test.txt").unwrap().ends_with("Test.txt"));
        assert!(copy_unique(&source, &output, "Test.txt").unwrap().ends_with("Test_2.txt"));
        assert_eq!(fs::read(&source).unwrap(), b"HELLO");
        fs::remove_dir_all(path).unwrap();
    }
}

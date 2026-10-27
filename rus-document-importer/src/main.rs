#![cfg_attr(windows, windows_subsystem = "windows")]
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

const CSV_NAME: &str = "component_import.csv";
const HEADER: &str = "Name;Number;Regulatory;File;Description;Category;DateCreated;DateUpdated;Hypertext;Graphic;DateEDS;FioEDS;PositionEDS;EDS";
const REGULATORY: &str = "Нет";

fn main() {
    if let Err(error) = run() {
        if let Ok(exe) = std::env::current_exe() {
            if let Some(dir) = exe.parent() {
                let _ = fs::write(dir.join("rus_importer_error.log"), error.to_string());
            }
        }
    }
}
fn run() -> io::Result<()> {
    let exe = std::env::current_exe()?;
    let dir = exe.parent().ok_or_else(|| io::Error::other("Не удалось определить папку EXE"))?;
    run_in(dir, &exe)
}
fn run_in(directory: &Path, exe: &Path) -> io::Result<()> {
    let (output, archive) = create_destination(directory)?;
    let mut rows = Vec::<(String, String)>::new();
    let mut issues = Vec::<String>::new();
    let mut file_paths = Vec::<PathBuf>::new();
    for result in fs::read_dir(directory)? {
        let entry = match result {
            Ok(entry) => entry,
            Err(error) => { issues.push(error.to_string()); continue; }
        };
        let path = entry.path();
        let meta = match entry.file_type() {
            Ok(meta) => meta,
            Err(error) => { issues.push(format!("{}: {}", path.display(), error)); continue; }
        };
        if !meta.is_file() || path == exe || path == archive { continue; }
        let original = entry.file_name().to_string_lossy().into_owned();
        if !original.chars().any(is_russian) || skip_generated(&original) { continue; }
        let (stem, _) = split_filename(&original);
        let human_name = stem.trim();
        if human_name.is_empty() { continue; }
        let proposed = transliterate_filename(&original);
        match copy_unique(&path, &output, &proposed) {
            Ok(destination) => {
                if let Some(name) = destination.file_name().and_then(|n| n.to_str()) {
                    rows.push((human_name.to_owned(), name.to_owned()));
                    file_paths.push(destination);
                }
            }
            Err(error) => issues.push(format!("{}: {}", original, error)),
        }
    }
    rows.sort_by(|left, right| left.0.to_lowercase().cmp(&right.0.to_lowercase()));
    file_paths.sort_by(|left, right| left.file_name().cmp(&right.file_name()));
    let csv = output.join(CSV_NAME);
    write_csv(&csv, &rows)?;
    write_archive(&archive, &csv, &file_paths)?;
    if !issues.is_empty() {
        fs::write(output.join("errors.log"), issues.join("\n"))?;
    }
    Ok(())
}
fn create_destination(directory: &Path) -> io::Result<(PathBuf, PathBuf)> {
    for i in 1..=100_000usize {
        let folder = if i == 1 { "Готовые".to_owned() } else { format!("Готовые_{}", i) };
        let dst = directory.join(&folder);
        let zip = directory.join(format!("{}.zip", folder));
        if dst.exists() || zip.exists() { continue; }
        match fs::create_dir(&dst) {
            Ok(()) => return Ok((dst, zip)),
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Слишком много папок результатов"))
}
fn is_russian(c: char) -> bool { matches!(c, 'А'..='я' | 'Ё' | 'ё') }
fn skip_generated(name: &str) -> bool {
    let lower = name.to_lowercase();
    lower.starts_with("готовые") && lower.ends_with(".zip")
        || lower.ends_with(".exe")
        || lower.ends_with(".dll")
        || lower.starts_with("component_import") && lower.ends_with(".csv")
}
fn split_filename(name: &str) -> (&str, Option<&str>) {
    match name.rsplit_once('.') {
        Some((stem, extension)) if !stem.is_empty() && !extension.is_empty() => (stem, Some(extension)),
        _ => (name, None),
    }
}
fn latin(c: char) -> Option<&'static str> {
    Some(match c {
        'а' => "a", 'б' => "b", 'в' => "v", 'г' => "g", 'д' => "d", 'е' => "e", 'ё' => "yo",
        'ж' => "zh", 'з' => "z", 'и' => "i", 'й' => "y", 'к' => "k", 'л' => "l", 'м' => "m",
        'н' => "n", 'о' => "o", 'п' => "p", 'р' => "r", 'с' => "s", 'т' => "t", 'у' => "u",
        'ф' => "f", 'х' => "kh", 'ц' => "ts", 'ч' => "ch", 'ш' => "sh", 'щ' => "shch",
        'ъ' => "", 'ы' => "y", 'ь' => "", 'э' => "e", 'ю' => "yu", 'я' => "ya",
        'А' => "A", 'Б' => "B", 'В' => "V", 'Г' => "G", 'Д' => "D", 'Е' => "E", 'Ё' => "Yo",
        'Ж' => "Zh", 'З' => "Z", 'И' => "I", 'Й' => "Y", 'К' => "K", 'Л' => "L", 'М' => "M",
        'Н' => "N", 'О' => "O", 'П' => "P", 'Р' => "R", 'С' => "S", 'Т' => "T", 'У' => "U",
        'Ф' => "F", 'Х' => "Kh", 'Ц' => "Ts", 'Ч' => "Ch", 'Ш' => "Sh", 'Щ' => "Shch",
        'Ъ' => "", 'Ы' => "Y", 'Ь' => "", 'Э' => "E", 'Ю' => "Yu", 'Я' => "Ya",
        _ => return None,
    })
}
fn transliterate_stem(stem: &str) -> String {
    let mut out = String::new();
    for c in stem.chars() {
        if c.is_whitespace() || c == '-' || c == '_' {
            if !out.is_empty() && !out.ends_with('-') { out.push('-'); }
        } else if let Some(part) = latin(c) {
            out.push_str(part);
        } else if !matches!(c, '/' | '\\' | ':' | '*' | '?' | '"' | '<' | '>' | '|') && !c.is_control() {
            out.push(c);
        }
    }
    out.trim_end_matches(['-', '.', ' ']).to_owned()
}
fn max_utf16(value: &str, limit: usize) -> String {
    let mut sum = 0;
    let mut out = String::new();
    for c in value.chars() {
        if sum + c.len_utf16() > limit { break; }
        sum += c.len_utf16();
        out.push(c);
    }
    out
}
fn windows_reserved(stem: &str) -> bool {
    let base = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    matches!(base.as_str(), "CON" | "PRN" | "AUX" | "NUL") ||
        (base.len() == 4 && (base.starts_with("COM") || base.starts_with("LPT"))
         && matches!(base.as_bytes()[3], b'1'..=b'9'))
}
fn transliterate_filename(name: &str) -> String {
    let (stem, extension) = split_filename(name);
    let mut stem = transliterate_stem(stem);
    if stem.is_empty() { stem = "file".to_owned(); }
    if windows_reserved(&stem) { stem.push('_'); }
    let stem = max_utf16(&stem, 180);
    match extension {
        Some(extension) => {
            let extension = max_utf16(&transliterate_stem(extension), 50);
            if extension.is_empty() { stem } else { format!("{}.{}", stem, extension) }
        }
        None => stem,
    }
}
fn copy_unique(source: &Path, directory: &Path, name: &str) -> io::Result<PathBuf> {
    let (stem, extension) = split_filename(name);
    for index in 1..=100_000usize {
        let destination_name = if index == 1 {
            name.to_owned()
        } else {
            let suffix = format!("_{}", index);
            let short = max_utf16(stem, 180usize.saturating_sub(suffix.len()));
            if let Some(ext) = extension { format!("{}{}.{}", short, suffix, ext) }
            else { format!("{}{}", short, suffix) }
        };
        let destination = directory.join(destination_name);
        match OpenOptions::new().write(true).create_new(true).open(&destination) {
            Ok(mut file) => {
                let result = (|| {
                    let mut input = File::open(source)?;
                    io::copy(&mut input, &mut file)?;
                    file.flush()
                })();
                if let Err(error) = result {
                    drop(file);
                    let _ = fs::remove_file(&destination);
                    return Err(error);
                }
                return Ok(destination);
            },
            Err(error) if error.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(error) => return Err(error),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Слишком много совпадений имён"))
}
fn quote_csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn write_csv(destination: &Path, rows: &[(String, String)]) -> io::Result<()> {
    let mut csv = BufWriter::new(File::create(destination)?);
    csv.write_all(b"\xEF\xBB\xBF")?;
    writeln!(csv, "{}", HEADER)?;
    for (name, file) in rows {
        let fields = [name.as_str(), "", REGULATORY, file.as_str(), "", "", "", "", "", "", "", "", "", ""];
        let line = fields.iter().map(|v| quote_csv(v)).collect::<Vec<_>>().join(";");
        writeln!(csv, "{}", line)?;
    }
    csv.flush()
}
fn write_archive(archive: &Path, csv: &Path, files: &[PathBuf]) -> io::Result<()> {
    let temp = archive.with_extension("zip.partial");
    let result = (|| -> io::Result<()> {
        let writer = File::create(&temp)?;
        let mut zip = ZipWriter::new(writer);
        let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
        for path in std::iter::once(csv).chain(files.iter().map(PathBuf::as_path)) {
            let basename = path.file_name().and_then(|n| n.to_str())
                .ok_or_else(|| io::Error::other("Недопустимое имя файла в ZIP"))?;
            zip.start_file(basename, options).map_err(io::Error::other)?;
            let mut input = File::open(path)?;
            let mut buffer = [0u8; 64 * 1024];
            loop {
                let count = input.read(&mut buffer)?;
                if count == 0 { break; }
                zip.write_all(&buffer[..count])?;
            }
        }
        zip.finish().map_err(io::Error::other)?;
        Ok(())
    })();
    if let Err(error) = result { let _ = fs::remove_file(temp); return Err(error); }
    fs::rename(temp, archive)
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn naming_is_correct() {
        assert_eq!(transliterate_filename("Приказ о зачислении.pdf"), "Prikaz-o-zachislenii.pdf");
        assert_eq!(transliterate_filename("Годовой отчёт 2026.docx"), "Godovoy-otchyot-2026.docx");
        assert_eq!(transliterate_filename("Ёжик  - щука.JPG"), "Yozhik-shchuka.JPG");
        assert_eq!(transliterate_filename("Положение_об_аттестации.pdf"), "Polozhenie-ob-attestatsii.pdf");
        assert_eq!(quote_csv("Имя; \"цитата\""), "\"Имя; \"\"цитата\"\"\"");
    }
    #[test]
    fn produces_real_import_zip_and_unchanged_sources() {
        let root = std::env::temp_dir().join(format!("rus_importer_test_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let input = root.join("Приказ о зачислении.pdf");
        fs::write(&input, b"a pretend pdf").unwrap();
        fs::write(root.join("latin.txt"), b"untouched").unwrap();
        let exe = root.join("test.exe");
        fs::write(&exe, b"exe").unwrap();
        run_in(&root, &exe).unwrap();
        let csv = fs::read(root.join("Готовые/component_import.csv")).unwrap();
        assert!(csv.starts_with(b"\xEF\xBB\xBF"));
        let text = String::from_utf8(csv).unwrap();
        assert!(text.contains("\"Приказ о зачислении\";\"\";\"Нет\";\"Prikaz-o-zachislenii.pdf\""));
        assert_eq!(fs::read(root.join("Готовые/Prikaz-o-zachislenii.pdf")).unwrap(), b"a pretend pdf");
        assert_eq!(fs::read(input).unwrap(), b"a pretend pdf");
        let zfile = File::open(root.join("Готовые.zip")).unwrap();
        let mut archive = zip::ZipArchive::new(zfile).unwrap();
        assert_eq!(archive.len(), 2);
        assert!(archive.by_name("component_import.csv").is_ok());
        assert!(archive.by_name("Prikaz-o-zachislenii.pdf").is_ok());
        run_in(&root, &exe).unwrap();
        assert!(root.join("Готовые_2.zip").exists());
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn collision_detection() {
        let root = std::env::temp_dir().join(format!("rus_importer_collision_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let src = root.join("Русский.txt");
        fs::write(&src, b"hello").unwrap();
        assert_eq!(copy_unique(&src, &root, "Russkiy.txt").unwrap().file_name().unwrap(), "Russkiy.txt");
        assert_eq!(copy_unique(&src, &root, "Russkiy.txt").unwrap().file_name().unwrap(), "Russkiy_2.txt");
        fs::remove_dir_all(root).unwrap();
    }
}

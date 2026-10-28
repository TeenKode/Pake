#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

const HEADERS: [&str; 14] = [
    "Name", "Number", "Regulatory", "File", "Description", "Category",
    "DateCreated", "DateUpdated", "Hypertext", "Graphic", "DateEDS",
    "FioEDS", "PositionEDS", "EDS",
];
const LABELS: [&str; 14] = [
    "Название", "Номер", "Нормативный", "Файл", "Описание", "Категория",
    "Дата создания", "Дата изменения", "Гипертекст", "Графика",
    "Дата ЭП", "ФИО подписанта", "Должность", "Электронная подпись",
];
const COLUMN_WIDTHS: [f32; 14] = [
    235.0, 105.0, 115.0, 235.0, 280.0, 200.0, 150.0,
    150.0, 210.0, 195.0, 150.0, 180.0, 200.0, 190.0,
];
const PROJECT_NAME: &str = "RusDocumentImporter.project.json";
const PAGE_SIZE: usize = 45;

#[derive(Clone, Serialize, Deserialize)]
struct Document {
    source: PathBuf,
    fields: [String; 14],
    included: bool,
}

#[derive(Serialize, Deserialize)]
struct Project {
    version: u32,
    folder: PathBuf,
    rows: Vec<Document>,
}

fn is_cyrillic(ch: char) -> bool {
    matches!(ch, 'А'..='я' | 'Ё' | 'ё')
}

fn translit_char(ch: char) -> Option<&'static str> {
    Some(match ch {
        'а'=>"a", 'б'=>"b", 'в'=>"v", 'г'=>"g", 'д'=>"d", 'е'=>"e", 'ё'=>"yo",
        'ж'=>"zh", 'з'=>"z", 'и'=>"i", 'й'=>"y", 'к'=>"k", 'л'=>"l", 'м'=>"m",
        'н'=>"n", 'о'=>"o", 'п'=>"p", 'р'=>"r", 'с'=>"s", 'т'=>"t", 'у'=>"u",
        'ф'=>"f", 'х'=>"kh", 'ц'=>"ts", 'ч'=>"ch", 'ш'=>"sh", 'щ'=>"shch",
        'ъ'=>"", 'ы'=>"y", 'ь'=>"", 'э'=>"e", 'ю'=>"yu", 'я'=>"ya",
        'А'=>"A", 'Б'=>"B", 'В'=>"V", 'Г'=>"G", 'Д'=>"D", 'Е'=>"E", 'Ё'=>"Yo",
        'Ж'=>"Zh", 'З'=>"Z", 'И'=>"I", 'Й'=>"Y", 'К'=>"K", 'Л'=>"L", 'М'=>"M",
        'Н'=>"N", 'О'=>"O", 'П'=>"P", 'Р'=>"R", 'С'=>"S", 'Т'=>"T", 'У'=>"U",
        'Ф'=>"F", 'Х'=>"Kh", 'Ц'=>"Ts", 'Ч'=>"Ch", 'Ш'=>"Sh", 'Щ'=>"Shch",
        'Ъ'=>"", 'Ы'=>"Y", 'Ь'=>"", 'Э'=>"E", 'Ю'=>"Yu", 'Я'=>"Ya",
        _ => return None,
    })
}

fn translit_stem(text: &str) -> String {
    let mut output = String::new();
    for ch in text.chars() {
        if ch.is_whitespace() || ch == '-' || ch == '_' {
            if !output.is_empty() && !output.ends_with('-') { output.push('-'); }
        } else if let Some(part) = translit_char(ch) {
            output.push_str(part);
        } else if !ch.is_control() && !matches!(ch, '/'|'\\'|':'|'*'|'?'|'"'|'<'|'>'|'|') {
            output.push(ch);
        }
    }
    output.trim_end_matches(['-', '.', ' ']).to_string()
}

fn name_parts(filename: &str) -> (&str, Option<&str>) {
    match filename.rsplit_once('.') {
        Some((stem, ext)) if !stem.is_empty() && !ext.is_empty() => (stem, Some(ext)),
        _ => (filename, None),
    }
}
fn crop_utf16(value: &str, limit: usize) -> String {
    let mut out = String::new();
    let mut count = 0;
    for ch in value.chars() {
        if count + ch.len_utf16() > limit { break; }
        count += ch.len_utf16();
        out.push(ch);
    }
    out
}
fn reserved_name(stem: &str) -> bool {
    let first = stem.split('.').next().unwrap_or(stem).to_ascii_uppercase();
    matches!(first.as_str(), "CON"|"PRN"|"AUX"|"NUL") ||
        (first.len() == 4 && (first.starts_with("COM") || first.starts_with("LPT"))
          && matches!(first.as_bytes()[3], b'1'..=b'9'))
}
fn transliterate_filename(filename: &str) -> String {
    let (stem, ext) = name_parts(filename);
    let mut stem = translit_stem(stem);
    if stem.is_empty() { stem = "file".to_owned(); }
    if reserved_name(&stem) { stem.push('_'); }
    let stem = crop_utf16(&stem, 180);
    match ext {
        Some(ext) => {
            let ext = crop_utf16(&translit_stem(ext), 50);
            if ext.is_empty() { stem } else { format!("{stem}.{ext}") }
        }
        None => stem,
    }
}
fn number_suffix(name: &str, index: usize) -> String {
    if index == 1 { return name.to_owned(); }
    let (stem, ext) = name_parts(name);
    let suffix = format!("_{index}");
    let stem = crop_utf16(stem, 180usize.saturating_sub(suffix.len()));
    match ext { Some(ext) => format!("{stem}{suffix}.{ext}"), None => format!("{stem}{suffix}") }
}

fn doc_extension(ext: &str) -> bool {
    matches!(ext, "pdf"|"doc"|"docx"|"odt"|"rtf"|"txt"|"xls"|"xlsx"|"ods"|"csvx"|
        "ppt"|"pptx"|"odp"|"jpg"|"jpeg"|"png"|"bmp"|"tif"|"tiff"|"gif"|"webp"|
        "svg"|"xml"|"html"|"htm"|"mp3"|"mp4"|"wav"|"avi"|"mov"|"dwg"|"dxf")
}

fn scan(folder: &Path, executable: &Path) -> io::Result<Vec<Document>> {
    let mut entries: Vec<_> = fs::read_dir(folder)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    let mut rows = Vec::new();
    let mut used_names = HashSet::new();
    for entry in entries {
        let source = entry.path();
        if source == executable { continue; }
        let filename = entry.file_name().to_string_lossy().into_owned();
        let lower = filename.to_lowercase();
        if lower == "component_import.csv" || lower == PROJECT_NAME.to_lowercase()
            || lower.starts_with("готовые") && lower.ends_with(".zip") { continue; }
        let (stem, ext) = name_parts(&filename);
        let extension = ext.unwrap_or("").to_lowercase();
        if matches!(extension.as_str(), "exe"|"dll"|"zip"|"json"|"log"|"bak"|"tmp"|"bat"|"cmd"|"ps1") { continue; }
        if !filename.chars().any(is_cyrillic) && !doc_extension(&extension) { continue; }
        let proposed = transliterate_filename(&filename);
        let mut i = 1;
        let resulting_name = loop {
            let candidate = number_suffix(&proposed, i);
            if used_names.insert(candidate.to_lowercase()) { break candidate; }
            i += 1;
        };
        let mut fields = std::array::from_fn(|_| String::new());
        fields[0] = stem.trim().to_owned();
        fields[2] = "Нет".to_owned();
        fields[3] = resulting_name;
        rows.push(Document { source, fields, included: true });
    }
    Ok(rows)
}

// The target importer's working example uses bare values for ordinary cells.
// Quote only if required for valid semicolon-delimited CSV.
fn quote_csv(value: &str) -> String {
    if value.chars().any(|c| matches!(c, ';' | '"' | '\r' | '\n')) {
        format!("\"{}\"", value.replace('"', "\"\""))
    } else {
        value.to_owned()
    }
}
fn csv_field(index: usize, value: &str) -> String {
    if index == 2 {
        // Regulatory: canonical strings as in the successfully imported sample.
        let canonical = match value.trim().to_lowercase().as_str() {
            "да" | "yes" | "true" | "1" => "Да",
            "нет" | "no" | "false" | "0" => "Нет",
            _ => value,
        };
        return quote_csv(canonical);
    }
    quote_csv(value)
}
// Fill fields present in every row of a verified successful import.
// Existing user-entered values are never overwritten.
fn fill_missing_metadata(rows: &mut [Document]) -> usize {
    let mut count = 0;
    for doc in rows.iter_mut().filter(|d| d.included) {
        if doc.fields[4].trim().is_empty() {
            let name = doc.fields[0].trim();
            if !name.is_empty() {
                doc.fields[4] = if name.starts_with("РП ") || name.starts_with("Рабочая программа") {
                    "Рабочая программа".to_string()
                } else {
                    name.to_string()
                };
                count += 1;
            }
        }
        if doc.fields[7].trim().is_empty() && !doc.fields[6].trim().is_empty() {
            doc.fields[7] = doc.fields[6].clone();
            count += 1;
        }
    }
    count
}
fn csv_bytes(rows: &[Document]) -> Vec<u8> {
    let mut csv = Vec::new();
    csv.extend_from_slice(HEADERS.join(";").as_bytes());
    csv.extend_from_slice(b"\r\n");
    for doc in rows.iter().filter(|row| row.included) {
        let row = doc.fields.iter().enumerate().map(|(i, field)| csv_field(i, field)).collect::<Vec<_>>().join(";");
        csv.extend_from_slice(row.as_bytes());
        csv.extend_from_slice(b"\r\n");
    }
    csv
}
fn valid_filename(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." ||
       name.ends_with([' ', '.']) || name.chars().any(|c| c.is_control() ||
        matches!(c, '/'|'\\'|':'|'*'|'?'|'"'|'<'|'>'|'|')) { return false; }
    let (stem, _) = name_parts(name);
    !reserved_name(stem) && name.encode_utf16().count() <= 240
}
fn validate_rows(rows: &[Document]) -> Result<(), String> {
    let mut used: HashMap<String, usize> = HashMap::new();
    let mut count = 0usize;
    for (index, row) in rows.iter().enumerate() {
        if !row.included { continue; }
        count += 1;
        let name = row.fields[3].trim();
        if !valid_filename(name) {
            return Err(format!("Строка {}: в колонке «Файл» недопустимое имя: {}", index+1, name));
        }
        if let Some(first) = used.insert(name.to_lowercase(), index+1) {
            return Err(format!("Строки {} и {}: одинаковое имя файла «{}». Измените колонку «Файл».", first, index+1, name));
        }
        if !row.source.is_file() {
            return Err(format!("Не найден исходный файл строки {}: {}", index+1, row.source.display()));
        }
    }
    if count == 0 { return Err("Нет отмеченных файлов для экспорта.".to_owned()); }
    Ok(())
}
fn new_output(folder: &Path) -> io::Result<(PathBuf, PathBuf)> {
    for index in 1..=100000 {
        let stem = if index == 1 { "Готовые".to_owned() } else { format!("Готовые_{index}") };
        let dir = folder.join(&stem);
        let archive = folder.join(format!("{stem}.zip"));
        if dir.exists() || archive.exists() { continue; }
        match fs::create_dir(&dir) {
            Ok(()) => return Ok((dir, archive)),
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e),
        }
    }
    Err(io::Error::new(io::ErrorKind::AlreadyExists, "Исчерпаны имена папок экспорта"))
}
fn make_zip(archive: &Path, folder: &Path, files: &[String]) -> io::Result<()> {
    // Read/write is required when adjusting the finished ZIP central directory.
    let file = OpenOptions::new().read(true).write(true).create_new(true).open(archive)?;
    let mut zip = ZipWriter::new(file);
    let options = FileOptions::default().compression_method(CompressionMethod::Deflated);
    for name in std::iter::once("component_import.csv").chain(files.iter().map(String::as_str)) {
        zip.start_file(name, options).map_err(io::Error::other)?;
        let mut input = File::open(folder.join(name))?;
        let mut buffer = [0u8; 64*1024];
        loop {
            let n = input.read(&mut buffer)?;
            if n == 0 { break; }
            zip.write_all(&buffer[..n])?;
        }
    }
    let mut file = zip.finish().map_err(io::Error::other)?;
    patch_zip_windows_metadata(&mut file)?;
    Ok(())
}
// Older ZIP importers can require Windows-style central-directory metadata,
// as seen in the verified working ZIP. Only the ZIP metadata is modified.
fn patch_zip_windows_metadata(file: &mut File) -> io::Result<()> {
    let end = file.seek(SeekFrom::End(0))?;
    let n = end.min(65_557) as usize;
    file.seek(SeekFrom::End(-(n as i64)))?;
    let mut tail = vec![0u8; n];
    file.read_exact(&mut tail)?;
    let eocd = tail.windows(4).rposition(|bytes| bytes == b"PK\x05\x06")
        .ok_or_else(|| io::Error::other("ZIP end-of-central-directory not found"))?;
    if eocd + 22 > n { return Err(io::Error::other("Incomplete ZIP footer")); }
    let u16le = |offset: usize| u16::from_le_bytes([tail[offset], tail[offset+1]]);
    let entries = u16le(eocd + 10) as usize;
    let center = u32::from_le_bytes(tail[eocd+16..eocd+20].try_into().unwrap());
    if entries == 65535 || center == u32::MAX { return Ok(()); } // ZIP64: leave unchanged
    let mut at = center as u64;
    for _ in 0..entries {
        let mut header = [0u8; 46];
        file.seek(SeekFrom::Start(at))?;
        file.read_exact(&mut header)?;
        if &header[..4] != b"PK\x01\x02" {
            return Err(io::Error::other("Invalid ZIP central directory"));
        }
        // version-made-by: low byte is ZIP specification version, high byte is host OS
        file.seek(SeekFrom::Start(at + 5))?;
        file.write_all(&[0])?;  // 0 = MS-DOS / Windows
        file.seek(SeekFrom::Start(at + 38))?;
        file.write_all(&0x20u32.to_le_bytes())?;  // DOS archive attribute
        let filename_len = u16::from_le_bytes([header[28], header[29]]) as u64;
        let extra_len = u16::from_le_bytes([header[30], header[31]]) as u64;
        let comment_len = u16::from_le_bytes([header[32], header[33]]) as u64;
        at += 46 + filename_len + extra_len + comment_len;
    }
    file.flush()
}
fn export(folder: &Path, rows: &[Document]) -> Result<PathBuf, String> {
    let mut prepared = rows.to_vec();
    fill_missing_metadata(&mut prepared);
    validate_rows(&prepared)?;
    let rows = prepared.as_slice();
    let (directory, zip_path) = new_output(folder).map_err(|e| e.to_string())?;
    let result = (|| -> io::Result<()> {
        let csv = csv_bytes(rows);
        fs::write(directory.join("component_import.csv"), &csv)?;
        let mut filenames = Vec::new();
        for row in rows.iter().filter(|row| row.included) {
            let filename = &row.fields[3];
            fs::copy(&row.source, directory.join(filename))?;
            filenames.push(filename.clone());
        }
        make_zip(&zip_path, &directory, &filenames)
    })();
    if let Err(error) = result {
        let _ = fs::remove_file(&zip_path);
        let _ = fs::remove_dir_all(&directory);
        return Err(format!("Не удалось сохранить файлы: {error}"));
    }
    Ok(zip_path)
}
fn bulk_edit(rows: &mut [Document], column: usize, value: &str, only_checked: bool) -> usize {
    if column >= 14 { return 0; }
    let mut updated = 0;
    for row in rows {
        if !only_checked || row.included {
            row.fields[column] = value.to_owned();
            updated += 1;
        }
    }
    updated
}
fn save_project(folder: &Path, rows: &[Document]) -> Result<PathBuf, String> {
    let project = Project { version: 1, folder: folder.to_owned(), rows: rows.to_vec() };
    let raw = serde_json::to_vec_pretty(&project).map_err(|e| e.to_string())?;
    let path = folder.join(PROJECT_NAME);
    let temporary = folder.join(format!("{PROJECT_NAME}.tmp"));
    fs::write(&temporary, raw).map_err(|e| e.to_string())?;
    fs::rename(&temporary, &path).map_err(|e| e.to_string())?;
    Ok(path)
}
fn load_project(path: &Path) -> Result<Project, String> {
    let raw = fs::read(path).map_err(|e| e.to_string())?;
    let parsed: Project = serde_json::from_slice(&raw).map_err(|e| e.to_string())?;
    if parsed.version != 1 { return Err("Неизвестная версия файла проекта".into()); }
    Ok(parsed)
}

#[path = "ui_compact.rs"]
mod ui;

fn main() -> eframe::Result {
    // Headless integration mode for Windows build validation; normal launches use GUI.
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 3 && args[1] == "--smoke-test-folder" {
        let folder = PathBuf::from(&args[2]);
        let rows = scan(&folder, &std::env::current_exe().unwrap_or_default()).unwrap();
        if rows.is_empty() { std::process::exit(3); }
        if export(&folder, &rows).is_err() { std::process::exit(4); }
        return Ok(());
    }
    ui::launch()
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn translit_and_exported_filename() {
        assert_eq!(transliterate_filename("Положение об аттестации.pdf"), "Polozhenie-ob-attestatsii.pdf");
        assert_eq!(transliterate_filename("Годовой отчёт 2026.docx"), "Godovoy-otchyot-2026.docx");
        assert_eq!(transliterate_filename("Ёжик  - щука.JPG"), "Yozhik-shchuka.JPG");
    }
    #[test]
    fn bulk_edit_every_row_and_checked_rows() {
        let mut rows = vec![
            Document { source: PathBuf::from("a.pdf"), fields: std::array::from_fn(|_| String::new()), included: true },
            Document { source: PathBuf::from("b.pdf"), fields: std::array::from_fn(|_| String::new()), included: false },
        ];
        assert_eq!(bulk_edit(&mut rows, 5, "Локальные акты", false), 2);
        assert_eq!(rows[0].fields[5], "Локальные акты");
        assert_eq!(rows[1].fields[5], "Локальные акты");
        assert_eq!(bulk_edit(&mut rows, 2, "Да", true), 1);
        assert_eq!(rows[0].fields[2], "Да");
        assert_eq!(rows[1].fields[2], "");
    }
    #[test]
    fn real_zip_and_csv_utf8_without_bom() {
        let root = std::env::temp_dir().join(format!("rus_importer_gui_test_{}",std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("Приказ №1.pdf");
        fs::write(&source,b"PDF_CONTENT").unwrap();
        let mut cols = std::array::from_fn(|_| String::new());
        cols[0] = "Приказ №1".into();
        cols[2] = "Да".into();
        cols[3] = "Prikaz-№1.pdf".into();
        cols[5] = "Локальные; \"нормативные\"".into();
        let rows = vec![Document { source: source.clone(), fields: cols, included: true }];
        let zip_path = export(&root, &rows).unwrap();
        let csv = fs::read(root.join("Готовые").join("component_import.csv")).unwrap();
        assert!(csv.starts_with(b"Name;Number;Regulatory;File;"));
        assert!(!csv.starts_with(&[0xEF,0xBB,0xBF]));
        let csv = String::from_utf8(csv).unwrap();
        assert!(csv.contains("Приказ №1"));
        assert!(csv.contains("\"Локальные; \"\"нормативные\"\"\""));
        assert_eq!(fs::read(&source).unwrap(), b"PDF_CONTENT");
        let zipfile = File::open(zip_path).unwrap();
        let mut z = zip::ZipArchive::new(zipfile).unwrap();
        assert!(z.by_name("component_import.csv").is_ok());
        assert!(z.by_name("Prikaz-№1.pdf").is_ok());
        drop(z);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn exports_working_template_style_and_canonical_regulatory() {
        let mut fields = std::array::from_fn(|_| String::new());
        fields[0] = "РП Алгебра 10-11 класс".into();
        fields[2] = "да".into();
        fields[3] = "RP-Algebra-10-11-klass.pdf".into();
        fields[5] = "Рабочие программы СОО".into();
        fields[6] = "01.09.2026".into();
        let rows = vec![Document {source: PathBuf::from("dummy.pdf"),fields,included:true}];
        let csv = String::from_utf8(csv_bytes(&rows)).unwrap();
        assert!(csv.starts_with("Name;Number;Regulatory;File;"));
        assert!(csv.contains("РП Алгебра 10-11 класс;;Да;RP-Algebra-10-11-klass.pdf"));
        assert!(!csv.contains("\"РП Алгебра 10-11 класс\""));
        assert!(!csv.contains("\"\""));
        assert_eq!(quote_csv("содержит;разделитель"), "\"содержит;разделитель\"");
    }
    #[test]
    fn fills_missing_description_and_date_updated_without_overwriting() {
        let mut fields = std::array::from_fn(|_| String::new());
        fields[0] = "РП Алгебра 10 класс".into();
        fields[2] = "Да".into();
        fields[3] = "RP-Algebra-10-klass.pdf".into();
        fields[6] = "01.09.2026".into();
        let mut rows = vec![Document {
            source: PathBuf::from("example.pdf"), fields, included: true
        }];
        assert_eq!(fill_missing_metadata(&mut rows), 2);
        assert_eq!(rows[0].fields[4], "Рабочая программа");
        assert_eq!(rows[0].fields[7], "01.09.2026");
        assert_eq!(fill_missing_metadata(&mut rows), 0);
        let csv = String::from_utf8(csv_bytes(&rows)).unwrap();
        assert!(csv.contains(";Рабочая программа;;01.09.2026;01.09.2026;"));
    }
    #[test]
    fn reject_duplicate_filename_after_bulk_change() {
        let mut fields = std::array::from_fn(|_| String::new());
        fields[3] = "same.pdf".to_owned();
        let doc = Document { source: PathBuf::from("unused"), fields, included:true };
        let mut another = doc.clone();
        another.source = PathBuf::from("unused2");
        assert!(validate_rows(&[doc, another]).is_err());
        assert!(!valid_filename("../evil.pdf"));
    }
}

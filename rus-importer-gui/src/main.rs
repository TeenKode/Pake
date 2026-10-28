#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, BufWriter, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use zip::write::FileOptions;
use zip::{CompressionMethod, ZipWriter};

const PROJECT_NAME: &str = "RusDocumentImporter.project.json";

#[derive(Clone, Serialize, Deserialize)]
struct Document {
    source: PathBuf,
    fields: Vec<String>,
    included: bool,
}

#[derive(Clone)]
struct TemplateSpec {
    path: PathBuf,
    headers: Vec<String>,
    defaults: Vec<String>,
    delimiter: u8,
    example: bool,
}

#[derive(Serialize, Deserialize)]
struct Project {
    version: u32,
    folder: PathBuf,
    #[serde(default)]
    headers: Vec<String>,
    #[serde(default)]
    template_path: PathBuf,
    rows: Vec<Document>,
}

fn column_index(headers: &[String], name: &str) -> Option<usize> {
    headers.iter().position(|h| h.trim().eq_ignore_ascii_case(name))
}
fn template_filename(folder: &Path) -> Option<PathBuf> {
    let mut candidates: Vec<PathBuf> = fs::read_dir(folder).ok()?
        .flatten()
        .filter(|entry| entry.file_type().map(|t| t.is_file()).unwrap_or(false))
        .map(|entry| entry.path())
        .filter(|p| p.extension().and_then(|v| v.to_str())
            .map(|s| s.eq_ignore_ascii_case("csv")).unwrap_or(false))
        .collect();
    candidates.sort_by_key(|p| p.file_name().unwrap_or_default().to_string_lossy().to_lowercase());
    if let Some(f) = candidates.iter().find(|p| p.file_name().unwrap_or_default()
        .to_string_lossy().eq_ignore_ascii_case("component_import.csv")) { return Some(f.clone()); }
    if let Some(f) = candidates.iter().find(|p| p.file_name().unwrap_or_default()
        .to_string_lossy().to_lowercase().starts_with("component_import")) { return Some(f.clone()); }
    if let Some(f) = candidates.iter().find(|p| {
        let n = p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
        n.contains("шаблон") || n.contains("template")
    }) { return Some(f.clone()); }
    if candidates.len() == 1 { return candidates.into_iter().next(); }
    None
}
fn sniff_delimiter(raw: &[u8]) -> u8 {
    let mut counts = [0usize; 3];
    let mut quoted = false;
    let mut i = 0;
    while i < raw.len() {
        let c = raw[i];
        if c == b'"' {
            if quoted && raw.get(i+1) == Some(&b'"') { i += 1; }
            else { quoted = !quoted; }
        } else if !quoted {
            if c == b'\n' || c == b'\r' { break; }
            match c { b';' => counts[0] += 1, b'\t' => counts[1] += 1, b',' => counts[2] += 1, _ => {} }
        }
        i += 1;
    }
    let winner = counts.iter().enumerate().max_by_key(|(i,c)| (**c, 2-*i)).unwrap().0;
    [b';', b'\t', b','][winner]
}
fn read_template(path: &Path) -> Result<TemplateSpec,String> {
    let raw = fs::read(path).map_err(|e| format!("Не удалось прочитать шаблон: {e}"))?;
    let raw = raw.strip_prefix(&[0xEF,0xBB,0xBF]).unwrap_or(&raw);
    std::str::from_utf8(raw).map_err(|e| format!("Шаблон должен быть UTF-8: {e}"))?;
    let delimiter = sniff_delimiter(raw);
    let mut reader = csv::ReaderBuilder::new().has_headers(false).delimiter(delimiter)
        .flexible(true).from_reader(raw);
    let mut records = reader.records();
    let first = records.next().ok_or("В шаблоне отсутствует строка заголовков")?
        .map_err(|e| format!("Ошибка чтения заголовков CSV: {e}"))?;
    let headers: Vec<String> = first.iter().map(|s| s.trim().to_owned()).collect();
    if headers.is_empty() || headers.len() > 128 || headers.iter().any(|s| s.is_empty()) {
        return Err("Шаблон должен содержать от 1 до 128 непустых заголовков".into());
    }
    let mut seen = HashSet::new();
    if headers.iter().any(|h| !seen.insert(h.to_lowercase())) {
        return Err("В шаблоне есть повторяющиеся названия колонок".into());
    }
    if column_index(&headers, "Name").is_none() || column_index(&headers, "File").is_none() {
        return Err("В CSV-шаблоне должны быть колонки Name и File".into());
    }
    let rows = records.map(|r| r.map_err(|e|e.to_string()))
        .collect::<Result<Vec<_>,_>>()
        .map_err(|e| format!("Ошибка чтения строк шаблона: {e}"))?;
    let mut defaults = vec![String::new(); headers.len()];
    let example = rows.len() == 1 && (
        rows[0].iter().any(|v| v.trim().to_lowercase().starts_with("укажите "))
        || column_index(&headers,"File").and_then(|i| rows[0].get(i))
            .map(|v| v.eq_ignore_ascii_case("document.pdf")).unwrap_or(false)
    );
    if rows.len() == 1 {
        for (idx,value) in rows[0].iter().take(defaults.len()).enumerate() {
            if !example || headers[idx].eq_ignore_ascii_case("Regulatory") {
                defaults[idx] = value.to_owned();
            }
        }
    }
    if let Some(idx) = column_index(&headers,"Name") { defaults[idx].clear(); }
    if let Some(idx) = column_index(&headers,"File") { defaults[idx].clear(); }
    Ok(TemplateSpec {path:path.to_path_buf(),headers,defaults,delimiter,example})
}
fn restore_project_rows(project: Project, template: &TemplateSpec) -> Vec<Document> {
    project.rows.into_iter().filter_map(|mut doc| {
        if project.headers.is_empty() {
            if doc.fields.len() != template.headers.len() { return None; }
        } else {
            let previous = doc.fields;
            doc.fields = template.headers.iter().enumerate().map(|(i,name)| {
                project.headers.iter().position(|h| h.eq_ignore_ascii_case(name))
                    .and_then(|old|previous.get(old).cloned())
                    .unwrap_or_else(||template.defaults[i].clone())
            }).collect();
        }
        Some(doc)
    }).collect()
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

fn scan(folder: &Path, executable: &Path, template: &TemplateSpec) -> io::Result<Vec<Document>> {
    let mut entries: Vec<_> = fs::read_dir(folder)?
        .filter_map(Result::ok)
        .filter(|entry| entry.file_type().map(|ft| ft.is_file()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e| e.file_name().to_string_lossy().to_lowercase());
    let mut rows = Vec::new();
    let mut used_names = HashSet::new();
    let name_col = column_index(&template.headers,"Name").expect("validated template");
    let file_col = column_index(&template.headers,"File").expect("validated template");
    for entry in entries {
        let source = entry.path();
        if source == executable || source == template.path { continue; }
        let filename = entry.file_name().to_string_lossy().into_owned();
        let lower = filename.to_lowercase();
        if lower == PROJECT_NAME.to_lowercase()
            || lower.starts_with("готовые") && lower.ends_with(".zip") { continue; }
        let (stem, ext) = name_parts(&filename);
        let extension = ext.unwrap_or("").to_lowercase();
        if matches!(extension.as_str(),"exe"|"dll"|"zip"|"json"|"log"|"bak"|"tmp"|"bat"|"cmd"|"ps1"|"csv") { continue; }
        if !filename.chars().any(is_cyrillic) && !doc_extension(&extension) { continue; }
        let proposed = clean_import_filename(&transliterate_filename(&filename));
        let mut i = 1;
        let resulting_name = loop {
            let candidate = number_suffix(&proposed, i);
            if used_names.insert(candidate.to_lowercase()) { break candidate; }
            i += 1;
        };
        let mut fields = template.defaults.clone();
        fields[name_col] = stem.trim().to_owned();
        fields[file_col] = resulting_name;
        rows.push(Document { source, fields, included: true });
    }
    Ok(rows)
}

// Keep all template headings, their order, and their original delimiter.
fn csv_bytes(rows: &[Document], template: &TemplateSpec) -> Result<Vec<u8>,String> {
    let mut writer = csv::WriterBuilder::new().delimiter(template.delimiter)
        .terminator(csv::Terminator::CRLF)
        .quote_style(csv::QuoteStyle::Necessary)
        .from_writer(Vec::new());
    writer.write_record(&template.headers).map_err(|e|e.to_string())?;
    let regulatory = column_index(&template.headers,"Regulatory");
    for doc in rows.iter().filter(|row| row.included) {
        let mut values = doc.fields.clone();
        if let Some(idx) = regulatory {
            if let Some(v) = values.get_mut(idx) {
                *v = match v.trim().to_lowercase().as_str() {
                    "да"|"yes"|"true"|"1" => "Да".into(),
                    "нет"|"no"|"false"|"0" => "Нет".into(),
                    _ => v.clone()
                };
            }
        }
        writer.write_record(&values).map_err(|e|e.to_string())?;
    }
    writer.into_inner().map_err(|e|e.to_string())
}

fn clean_export_rows(rows: &mut [Document], template: &TemplateSpec) -> usize {
    let file_col = column_index(&template.headers,"File").expect("validated template");
    let mut count = 0;
    let mut used = HashSet::new();
    for doc in rows.iter_mut().filter(|doc|doc.included) {
        let base = clean_import_filename(&doc.fields[file_col]);
        let mut suffix = 1;
        let filename = loop {
            let candidate = number_suffix(&base,suffix);
            if used.insert(candidate.to_lowercase()) { break candidate; }
            suffix += 1;
        };
        if filename != doc.fields[file_col] { doc.fields[file_col] = filename; count += 1; }
    }
    count
}

fn valid_filename(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." ||
       name.ends_with([' ', '.']) || name.chars().any(|c| c.is_control() ||
        matches!(c, '/'|'\\'|':'|'*'|'?'|'"'|'<'|'>'|'|')) { return false; }
    let (stem, _) = name_parts(name);
    !reserved_name(stem) && name.encode_utf16().count() <= 240
}
fn validate_rows(rows: &[Document], template: &TemplateSpec) -> Result<(),String> {
    let file_col = column_index(&template.headers,"File").expect("validated template");
    let name_col = column_index(&template.headers,"Name").expect("validated template");
    let mut used: HashMap<String,usize> = HashMap::new();
    let mut count = 0usize;
    for (index,row) in rows.iter().enumerate() {
        if !row.included { continue; }
        count += 1;
        if row.fields.len() != template.headers.len() {
            return Err(format!("Строка {} не соответствует колонкам шаблона",index+1));
        }
        if row.fields[name_col].trim().is_empty() {
            return Err(format!("Строка {}: не заполнено поле Name",index+1));
        }
        let filename = row.fields[file_col].trim();
        if !valid_filename(filename) {
            return Err(format!("Строка {}: недопустимое имя File: {}",index+1,filename));
        }
        if let Some(first) = used.insert(filename.to_lowercase(),index+1) {
            return Err(format!("Строки {} и {}: одинаковое имя файла {}",first,index+1,filename));
        }
        if !row.source.is_file() {
            return Err(format!("Не найден исходный файл строки {}: {}",index+1,row.source.display()));
        }
    }
    if count == 0 { return Err("Нет отмеченных файлов для экспорта".into()); }
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
fn export(folder: &Path, rows: &[Document], template: &TemplateSpec) -> Result<PathBuf,String> {
    let mut prepared = rows.to_vec();
    clean_export_rows(&mut prepared, template);
    validate_rows(&prepared, template)?;
    let (directory, zip_path) = new_output(folder).map_err(|e|e.to_string())?;
    let result = (|| -> io::Result<()> {
        let csv = csv_bytes(&prepared,template).map_err(io::Error::other)?;
        fs::write(directory.join("component_import.csv"),&csv)?;
        let file_col = column_index(&template.headers,"File").expect("validated template");
        let mut files = Vec::new();
        for row in prepared.iter().filter(|row|row.included) {
            let filename = &row.fields[file_col];
            fs::copy(&row.source,directory.join(filename))?;
            files.push(filename.clone());
        }
        make_zip(&zip_path,&directory,&files)
    })();
    if let Err(e) = result {
        let _ = fs::remove_file(&zip_path);
        let _ = fs::remove_dir_all(&directory);
        return Err(format!("Не удалось создать архив: {e}"));
    }
    Ok(zip_path)
}
fn bulk_edit(rows: &mut [Document],column:usize,value:&str,only_checked:bool) -> usize {
    let mut changed = 0;
    for doc in rows.iter_mut().filter(|r|!only_checked || r.included) {
        if let Some(cell) = doc.fields.get_mut(column) {
            *cell = value.to_owned(); changed += 1;
        }
    }
    changed
}
fn save_project(folder: &Path, rows: &[Document], template: &TemplateSpec) -> Result<PathBuf,String> {
    let project = Project {
        version:2,folder:folder.to_path_buf(),headers:template.headers.clone(),
        template_path:template.path.clone(),rows:rows.to_vec(),
    };
    let bytes = serde_json::to_vec_pretty(&project).map_err(|e|e.to_string())?;
    let path = folder.join(PROJECT_NAME);
    let temp = folder.join(format!("{PROJECT_NAME}.tmp"));
    fs::write(&temp,bytes).map_err(|e|e.to_string())?;
    fs::rename(&temp,&path).map_err(|e|e.to_string())?;
    Ok(path)
}
fn load_project(path: &Path) -> Result<Project,String> {
    let raw = fs::read(path).map_err(|e|e.to_string())?;
    let project: Project = serde_json::from_slice(&raw).map_err(|e|e.to_string())?;
    if project.version != 1 && project.version != 2 { return Err("Неизвестная версия проекта".into()); }
    Ok(project)
}

#[path = "ui_compact.rs"]
mod ui;

fn main() -> eframe::Result {
    let args: Vec<_> = std::env::args_os().collect();
    if args.len() == 3 && args[1] == "--smoke-test-folder" {
        let folder = PathBuf::from(&args[2]);
        let template_path = template_filename(&folder).expect("CSV template not found");
        let template = read_template(&template_path).expect("Invalid CSV template");
        let rows = scan(&folder,&std::env::current_exe().unwrap_or_default(),&template).unwrap();
        if rows.is_empty() { std::process::exit(3); }
        if export(&folder,&rows,&template).is_err() { std::process::exit(4); }
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
    fn parentheses_removed_only_from_file_names() {
        assert_eq!(
            clean_import_filename("RP-Algebra-10-11-klass-(Bazovyy).pdf"),
            "RP-Algebra-10-11-klass-Bazovyy.pdf"
        );
        assert_eq!(
            clean_import_filename("RP-informatika-10-11-((Bazovyy)).pdf"),
            "RP-informatika-10-11-Bazovyy.pdf"
        );
        assert_eq!(
            clean_import_filename("RP-Geometriya-10-11-klass(Uglublennyy).pdf"),
            "RP-Geometriya-10-11-klass-Uglublennyy.pdf"
        );
        assert_eq!(clean_import_filename("Normal-file.pdf"), "Normal-file.pdf");
    }
    #[test]
    fn export_sanitizes_only_file_names_preserving_names_and_sources() {
        let root = std::env::temp_dir().join(format!("rus_importer_brackets_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source_a = root.join("РП Алгебра 10 (Базовый).pdf");
        let source_b = root.join("РП Алгебра 10 Базовый.pdf");
        fs::write(&source_a, b"unchanged file A").unwrap();
        fs::write(&source_b, b"unchanged file B").unwrap();
        let mut first = std::array::from_fn(|_| String::new());
        first[0] = "РП Алгебра 10 (Базовый)".into();
        first[2] = "Да".into();
        first[3] = "RP-Algebra-10-(Bazovyy).pdf".into();
        let mut second = std::array::from_fn(|_| String::new());
        second[0] = "РП Алгебра 10 Базовый".into();
        second[2] = "Да".into();
        second[3] = "RP-Algebra-10-Bazovyy.pdf".into();
        let rows = vec![
            Document {source: source_a.clone(),fields:first,included:true},
            Document {source: source_b.clone(),fields:second,included:true},
        ];
        let zip_path = export(&root, &rows).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(zip_path).unwrap()).unwrap();
        let mut csv = String::new();
        zip.by_name("component_import.csv").unwrap().read_to_string(&mut csv).unwrap();
        assert!(csv.contains("РП Алгебра 10 (Базовый);;Да;RP-Algebra-10-Bazovyy.pdf;"));
        assert!(csv.contains("РП Алгебра 10 Базовый;;Да;RP-Algebra-10-Bazovyy_2.pdf;"));
        for line in csv.lines().skip(1) {
            let fields: Vec<_> = line.split(';').collect();
            assert!(fields.len() >= 4);
            assert!(!fields[3].contains('(') && !fields[3].contains(')'));
        }
        assert_eq!(zip.by_name("RP-Algebra-10-Bazovyy.pdf").unwrap().size(), 16);
        assert_eq!(zip.by_name("RP-Algebra-10-Bazovyy_2.pdf").unwrap().size(), 16);
        assert_eq!(fs::read(&source_a).unwrap(), b"unchanged file A");
        assert_eq!(fs::read(&source_b).unwrap(), b"unchanged file B");
        assert_eq!(rows[0].fields[0], "РП Алгебра 10 (Базовый)");
        assert_eq!(rows[0].fields[3], "RP-Algebra-10-(Bazovyy).pdf");
        drop(zip);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn scan_keeps_parentheses_in_russian_name_but_strips_filename() {
        let root = std::env::temp_dir().join(format!("rus_importer_scan_brackets_{}", std::process::id()));
        let _ = fs::remove_dir_all(&root);
        fs::create_dir_all(&root).unwrap();
        let source = root.join("РП Математика (профильная).pdf");
        fs::write(&source, b"pdf").unwrap();
        let docs = scan(&root, &root.join("RusDocumentImporter.exe")).unwrap();
        assert_eq!(docs.len(), 1);
        assert_eq!(docs[0].fields[0], "РП Математика (профильная)");
        assert_eq!(docs[0].fields[3], "RP-Matematika-profilnaya.pdf");
        fs::remove_dir_all(root).unwrap();
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

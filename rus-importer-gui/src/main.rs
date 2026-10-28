#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui;
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File, OpenOptions};
use std::io::{self, Read, Seek, SeekFrom, Write};
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
    // Column mapping is optional and user-editable; CSV schemas never have required names.
    title_column: Option<usize>,
    asset_column: Option<usize>,
    example_rows: Vec<Vec<String>>,
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
    #[serde(default)]
    title_field: Option<String>,
    #[serde(default)]
    asset_field: Option<String>,
    #[serde(default)]
    mapping_configured: bool,
}

fn column_index(headers: &[String], name: &str) -> Option<usize> {
    headers.iter().position(|h| h.trim().eq_ignore_ascii_case(name))
}
/// Discover candidate templates by content and extension, not by a specific filename.
/// Multiple candidates must be selected explicitly in the GUI.
fn template_candidates(folder: &Path) -> Result<Vec<PathBuf>, String> {
    let entries = fs::read_dir(folder).map_err(|e| format!("Не удалось открыть папку: {e}"))?;
    let mut csv_files = Vec::new();
    let mut other_text = Vec::new();
    let mut excel_files = Vec::new();
    for entry in entries {
        let entry = entry.map_err(|e| format!("Ошибка чтения папки: {e}"))?;
        if !entry.file_type().map(|t|t.is_file()).unwrap_or(false) { continue; }
        let path = entry.path();
        let ext = path.extension().and_then(|e|e.to_str()).unwrap_or("").to_ascii_lowercase();
        match ext.as_str() {
            "csv" => csv_files.push(path),
            "tsv" | "txt" => other_text.push(path),
            "xlsx" | "xls" | "ods" => excel_files.push(path),
            _ => {}
        }
    }
    csv_files.sort_by_key(|p|p.file_name().unwrap_or_default().to_string_lossy().to_lowercase());
    other_text.sort_by_key(|p|p.file_name().unwrap_or_default().to_string_lossy().to_lowercase());
    // .txt files count as templates only when they have the required Name/File headers.
    csv_files.extend(other_text.into_iter().filter(|p|read_template(p).is_ok()));
    if csv_files.is_empty() && !excel_files.is_empty() {
        return Err(format!(
            "В папке найден шаблон Excel ({}), но нужен CSV. В Excel: Файл → Сохранить как → CSV UTF-8.",
            excel_files[0].file_name().unwrap_or_default().to_string_lossy()
        ));
    }
    if csv_files.is_empty() {
        return Err("CSV-шаблон не найден. Положите CSV с колонками Name и File рядом с документами или выберите его через «Шаблон CSV».".to_owned());
    }
    Ok(csv_files)
}
fn template_filename(folder: &Path) -> Option<PathBuf> {
    let candidates = template_candidates(folder).ok()?;
    candidates.iter().filter(|path|read_template(path).is_ok())
        .min_by_key(|p| {
            let n=p.file_name().unwrap_or_default().to_string_lossy().to_lowercase();
            if n=="component_import.csv" {0}
            else if n.starts_with("component_import") {1}
            else if n.contains("шаблон") || n.contains("template") {2}
            else {3}
        }).cloned().or_else(||candidates.into_iter().next())
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

    let rows = records.map(|r| r.map_err(|e|e.to_string()))
        .collect::<Result<Vec<_>,_>>()
        .map_err(|e| format!("Ошибка чтения строк шаблона: {e}"))?;
    // Rows in the provided template are independent examples/data records.
    // Never silently copy a person's fields into newly generated records.
    let example_rows: Vec<Vec<String>> = rows.iter()
        .filter(|row|row.iter().any(|cell|!cell.trim().is_empty()))
        .map(|row| {
            (0..headers.len()).map(|i|row.get(i).unwrap_or("").to_owned()).collect()
        }).collect();
    let defaults = vec![String::new(); headers.len()];
    // These are *optional suggestions*, never mandatory columns.
    let title_column = column_index(&headers,"Name");
    let asset_column = column_index(&headers,"File").or_else(||column_index(&headers,"Image"));
    let example = !example_rows.is_empty();
    Ok(TemplateSpec {path:path.to_path_buf(),headers,defaults,delimiter,example,
        title_column,asset_column,example_rows})
}
fn apply_project_mapping(project:&Project,template:&mut TemplateSpec){
    if project.mapping_configured {
        template.title_column=project.title_field.as_deref()
            .and_then(|name|column_index(&template.headers,name));
        template.asset_column=project.asset_field.as_deref()
            .and_then(|name|column_index(&template.headers,name));
    }
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

fn is_supported_asset_extension(ext: &str) -> bool {
    doc_extension(ext) || matches!(ext,"png"|"jpg"|"jpeg"|"gif"|"bmp"|"webp"|"tiff"|"tif"|"svg")
}
fn rows_from_template(template:&TemplateSpec) -> Vec<Document> {
    template.example_rows.iter().map(|record|Document {
        source:PathBuf::new(),fields:record.clone(),included:false,
    }).collect()
}
fn scan(folder: &Path, executable: &Path, template: &TemplateSpec) -> io::Result<Vec<Document>> {
    let Some(asset_col) = template.asset_column else { return Ok(Vec::new()); };
    let mut entries: Vec<_> = fs::read_dir(folder)?
        .filter_map(Result::ok)
        .filter(|entry|entry.file_type().map(|ft|ft.is_file()).unwrap_or(false))
        .collect();
    entries.sort_by_key(|e|e.file_name().to_string_lossy().to_lowercase());
    let mut rows=Vec::new();
    let mut used_names=HashSet::new();
    // The sample values, when present, suggest a file type without requiring
    // any particular column name. This avoids importing PDFs as staff photos.
    let mut sample_image=false;
    let mut sample_document=false;
    for sample in &template.example_rows {
        if let Some(v)=sample.get(asset_col) {
            let ext=Path::new(v).extension().and_then(|e|e.to_str())
                .unwrap_or("").to_ascii_lowercase();
            if matches!(ext.as_str(),"jpg"|"jpeg"|"png"|"gif"|"bmp"|"tiff"|"tif"|"webp"|"svg"){
                sample_image=true;
            }
            if doc_extension(&ext){sample_document=true;}
        }
    }
    for entry in entries {
        let source=entry.path();
        if source==executable || source==template.path {continue;}
        let filename=entry.file_name().to_string_lossy().into_owned();
        let extension=source.extension().and_then(|x|x.to_str()).unwrap_or("").to_lowercase();
        if !is_supported_asset_extension(&extension) {continue;}
        let image=matches!(extension.as_str(),"jpg"|"jpeg"|"png"|"gif"|"bmp"|"tiff"|"tif"|"webp"|"svg");
        if sample_image && !sample_document && !image {continue;}
        if sample_document && !sample_image && image {continue;}
        let (stem,_) = name_parts(&filename);
        let proposed=clean_import_filename(&filename);
        let mut index=1;
        let safe=loop {
            let candidate=number_suffix(&proposed,index);
            if used_names.insert(candidate.to_lowercase()){break candidate;}
            index+=1;
        };
        let mut fields=template.defaults.clone();
        if let Some(col)=template.title_column {
            fields[col]=stem.trim().to_owned();
        }
        fields[asset_col]=safe;
        rows.push(Document{source,fields,included:true});
    }
    Ok(rows)
}

// Keep all template headings, their order, and their original delimiter.
/// Build a filesystem and site-safe basename. Only ASCII Latin letters, digits,
/// '-' and '_' are allowed. Punctuation and spaces become single '-' separators.
/// Cyrillic text is transliterated so manually edited File fields are safe too.
fn safe_filename_part(raw: &str) -> String {
    let mut result = String::new();
    for character in raw.chars() {
        if let Some(latin) = translit_char(character) {
            result.push_str(latin);
        } else if character.is_ascii_alphanumeric() || character == '_' {
            result.push(character);
        } else {
            // Replace every unsupported character with a single word separator.
            if !result.is_empty() && !result.ends_with('-') && !result.ends_with('_') {
                result.push('-');
            }
        }
    }
    result.trim_matches('-').to_owned()
}

/// Only the separator before the extension may be a dot.
fn clean_import_filename(value: &str) -> String {
    let (stem, extension) = name_parts(value);
    let mut safe_stem = crop_utf16(&safe_filename_part(stem), 180);
    if safe_stem.is_empty() { safe_stem = "file".to_owned(); }
    if reserved_name(&safe_stem) { safe_stem.push('_'); }
    let extension = extension
        .map(|e| crop_utf16(&safe_filename_part(e), 50))
        .filter(|e| !e.is_empty());
    match extension {
        Some(e) => format!("{safe_stem}.{e}"),
        None => safe_stem,
    }
}

fn csv_bytes(rows: &[Document], template: &TemplateSpec) -> Result<Vec<u8>,String> {
    let mut writer = csv::WriterBuilder::new().delimiter(template.delimiter)
        .terminator(csv::Terminator::CRLF)
        .quote_style(csv::QuoteStyle::Necessary)
        .from_writer(Vec::new());
    writer.write_record(&template.headers).map_err(|e|e.to_string())?;
    for doc in rows.iter().filter(|row| row.included) {
        writer.write_record(&doc.fields).map_err(|e|e.to_string())?;
    }
    writer.into_inner().map_err(|e|e.to_string())
}

fn clean_export_rows(rows:&mut [Document],template:&TemplateSpec)->usize{
    let Some(file_col)=template.asset_column else {return 0;};
    let mut count=0;
    let mut used=HashSet::new();
    for doc in rows.iter_mut().filter(|doc|doc.included && !doc.source.as_os_str().is_empty()){
        let base=clean_import_filename(&doc.fields[file_col]);
        let mut index=1;
        let filename=loop{
            let candidate=number_suffix(&base,index);
            if used.insert(candidate.to_lowercase()){break candidate;}
            index+=1;
        };
        if filename!=doc.fields[file_col]{doc.fields[file_col]=filename;count+=1;}
    }
    count
}

fn valid_filename(name: &str) -> bool {
    if name.is_empty() || name == "." || name == ".." ||
       name.ends_with([' ', '.']) || name.chars().any(|c| c.is_control() ||
        matches!(c, '/'|'\\'|':'|'*'|'?'|'"'|'<'|'>'|'|')) { return false; }
    let (stem, _) = name_parts(name);
    !reserved_name(stem) && name.encode_utf16().count() <= 240
        && clean_import_filename(name) == name
}
fn validate_rows(rows:&[Document],template:&TemplateSpec)->Result<(),String>{
    let mut count=0;
    let mut used=HashMap::<String,usize>::new();
    for (index,row) in rows.iter().enumerate(){
        if !row.included {continue;}
        count+=1;
        if row.fields.len()!=template.headers.len(){
            return Err(format!("Строка {}: число полей не совпадает с шаблоном",index+1));
        }
        if row.source.as_os_str().is_empty(){continue;} // CSV-only records are valid.
        if !row.source.is_file(){
            return Err(format!("Не найден исходный файл: {}",row.source.display()));
        }
        let Some(col)=template.asset_column else{
            return Err("Для вложенных файлов выберите колонку имени файла. CSV-строки без файлов разрешены.".into());
        };
        let filename=row.fields[col].trim();
        if !valid_filename(filename){
            return Err(format!("Строка {}: недопустимое имя вложения: {}",index+1,filename));
        }
        if let Some(first)=used.insert(filename.to_lowercase(),index+1){
            return Err(format!("Строки {} и {}: одинаковое имя вложения: {}",first,index+1,filename));
        }
    }
    if count==0{return Err("Нет отмеченных строк для экспорта".into());}
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
fn export(folder:&Path,rows:&[Document],template:&TemplateSpec)->Result<PathBuf,String>{
    let mut prepared=rows.to_vec();
    clean_export_rows(&mut prepared,template);
    validate_rows(&prepared,template)?;
    let (directory,zip_path)=new_output(folder).map_err(|e|e.to_string())?;
    let result=(||->io::Result<()>{
        let csv=csv_bytes(&prepared,template).map_err(io::Error::other)?;
        fs::write(directory.join("component_import.csv"),&csv)?;
        let mut files=Vec::new();
        for row in prepared.iter().filter(|r|r.included && !r.source.as_os_str().is_empty()){
            let col=template.asset_column.expect("validated attached file");
            let filename=&row.fields[col];
            fs::copy(&row.source,directory.join(filename))?;
            files.push(filename.clone());
        }
        make_zip(&zip_path,&directory,&files)
    })();
    if let Err(e)=result{
        let _=fs::remove_file(&zip_path);
        let _=fs::remove_dir_all(&directory);
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
/// Make projects movable between computers and disks; v2 projects with old absolute
/// source paths are still supported when their basenames exist in the new folder.
fn relink_document_sources(rows: &mut [Document], folder: &Path) {
    for row in rows {
        if row.source.is_relative() {
            if let Some(name) = row.source.file_name() {
                row.source = folder.join(name);
            }
        } else if let Some(name) = row.source.file_name() {
            // Prefer the local copy even while the old location still exists.
            let candidate = folder.join(name);
            if candidate.is_file() { row.source = candidate; }
        }
    }
}
fn save_project(folder: &Path, rows: &[Document], template: &TemplateSpec) -> Result<PathBuf,String> {
    let mut rows = rows.to_vec();
    for row in &mut rows {
        if let Ok(relative) = row.source.strip_prefix(folder) {
            row.source = relative.to_path_buf();
        }
    }
    let template_path = template.path.strip_prefix(folder)
        .map(Path::to_path_buf).unwrap_or_else(|_| template.path.clone());
    let project = Project {
        version: 2, folder: PathBuf::from("."), headers: template.headers.clone(),
        template_path, rows,
        title_field:template.title_column.and_then(|i|template.headers.get(i).cloned()),
        asset_field:template.asset_column.and_then(|i|template.headers.get(i).cloned()),
        mapping_configured:true,
    };
    let bytes = serde_json::to_vec_pretty(&project).map_err(|e|e.to_string())?;
    let path = folder.join(PROJECT_NAME);
    let temporary = folder.join(format!("{PROJECT_NAME}.tmp"));
    let backup = folder.join(format!("{PROJECT_NAME}.bak"));
    fs::write(&temporary, bytes).map_err(|e| e.to_string())?;
    if path.is_file() {
        if backup.exists() {
            fs::remove_file(&backup).map_err(|e| e.to_string())?;
        }
        fs::rename(&path, &backup).map_err(|e| e.to_string())?;
    }
    match fs::rename(&temporary, &path) {
        Ok(()) => {
            // Backup is left only if cleanup fails; it cannot block future loading.
            let _ = fs::remove_file(&backup);
            Ok(path)
        }
        Err(error) => {
            if backup.is_file() { let _ = fs::rename(&backup, &path); }
            let _ = fs::remove_file(&temporary);
            Err(format!("Не удалось сохранить проект: {error}"))
        }
    }
}
fn load_project(path: &Path) -> Result<Project,String> {
    let backup = path.with_file_name(format!("{PROJECT_NAME}.bak"));
    let raw = fs::read(path).or_else(|_| fs::read(&backup)).map_err(|e|e.to_string())?;
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
    fn sandbox(label:&str)->PathBuf {
        let path=std::env::temp_dir().join(format!("rus_importer_v6_{}_{}",label,std::process::id()));
        let _=fs::remove_dir_all(&path);
        fs::create_dir_all(&path).unwrap();
        path
    }
    #[test]
    fn template_with_reordered_extra_columns_and_defaults() {
        let folder=sandbox("custom");
        let template_path=folder.join("component_import.csv");
        fs::write(&template_path,"Category;File;Name;Extra;Regulatory\r\nЛокальные;sample.pdf;Образец;Школа;да\r\n").unwrap();
        fs::write(folder.join("Приказ (новый).pdf"),b"PDF").unwrap();
        let template=read_template(&template_path).unwrap();
        assert_eq!(template.headers.len(),5);
        assert_eq!(template.delimiter,b';');
        let rows=scan(&folder,&folder.join("importer.exe"),&template).unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0].fields[2],"Приказ (новый)");
        assert_eq!(rows[0].fields[1],"Prikaz-novyy.pdf");
        assert_eq!(rows[0].fields[0],"");
        assert_eq!(rows[0].fields[3],"");
        assert_eq!(template.example_rows[0][3],"Школа");
        let archive=export(&folder,&rows,&template).unwrap();
        let mut zip=zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
        let mut csv=String::new();
        zip.by_name("component_import.csv").unwrap().read_to_string(&mut csv).unwrap();
        assert!(csv.starts_with("Category;File;Name;Extra;Regulatory\r\n"));
        assert!(csv.contains(";Prikaz-novyy.pdf;Приказ (новый);;\r\n"));
        assert!(zip.by_name("Prikaz-novyy.pdf").is_ok());
        drop(zip);fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn utf8_bom_input_and_tsv_template_delimiter() {
        let folder=sandbox("tsv");
        let path=folder.join("component_import.csv");
        fs::write(&path,b"\xEF\xBB\xBFFile\tName\tExtra\nold.pdf\tSample\tvalue\n").unwrap();
        let template=read_template(&path).unwrap();
        assert_eq!(template.delimiter,b'\t');
        let doc=Document {
            source:PathBuf::new(),included:true,
            fields:vec!["test.pdf".into(),"Справка".into(),"Текст".into()],
        };
        let csv=csv_bytes(&[doc],&template).unwrap();
        assert!(!csv.starts_with(&[0xEF,0xBB,0xBF]));
        assert!(String::from_utf8(csv).unwrap().starts_with("File\tName\tExtra\r\n"));
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn ignores_example_row_from_supplied_template() {
        let folder=sandbox("example");
        let path=folder.join("component_import (3).csv");
        fs::write(&path,"Name;Regulatory;File;Description;DateCreated\n\"Укажите название документа\";Да;document.pdf;\"Укажите описание документа\";01.02.2021\n").unwrap();
        let template=read_template(&path).unwrap();
        assert!(template.example);
        assert_eq!(template.defaults[3],"");
        assert_eq!(template.defaults[4],"");
        assert_eq!(template.defaults[1],"");
        assert_eq!(template.example_rows[0][1],"Да");
        assert_eq!(rows_from_template(&template).len(),1);
        assert!(!rows_from_template(&template)[0].included);
        assert_eq!(template_filename(&folder).unwrap(),path);
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn remaps_project_fields_when_headers_are_reordered() {
        let old=Project {
            version:2,folder:PathBuf::from("."),template_path:PathBuf::from("old.csv"),
            headers:vec!["Name".into(),"File".into(),"Extra".into()],
            rows:vec![Document {
                source:PathBuf::from("sample.pdf"),fields:vec!["Имя".into(),"a.pdf".into(),"сохранено".into()],
                included:true,
            }],
            title_field:Some("Name".into()),asset_field:Some("File".into()),
            mapping_configured:true,
        };
        let template=TemplateSpec {
            path:PathBuf::from("new.csv"),
            headers:vec!["Extra".into(),"File".into(),"Name".into(),"NewColumn".into()],
            defaults:vec![String::new();4],delimiter:b';',example:false,
            title_column:Some(2),asset_column:Some(1),example_rows:vec![],
        };
        let rows=restore_project_rows(old,&template);
        assert_eq!(rows[0].fields,vec!["сохранено","a.pdf","Имя",""]);
    }
    #[test]
    fn parentheses_are_removed_only_from_filename() {
        let folder=sandbox("brackets");
        let path=folder.join("component_import.csv");
        fs::write(&path,"File;Name;Other\n").unwrap();
        let template=read_template(&path).unwrap();
        let source=folder.join("Алгебра (Базовый).pdf");
        fs::write(&source,b"original bytes").unwrap();
        let rows=scan(&folder,&folder.join("app.exe"),&template).unwrap();
        assert_eq!(rows.len(),1);
        assert_eq!(rows[0].fields[1],"Алгебра (Базовый)");
        assert_eq!(rows[0].fields[0],"Algebra-Bazovyy.pdf");
        let archive=export(&folder,&rows,&template).unwrap();
        let mut zip=zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
        let mut csv=String::new();
        zip.by_name("component_import.csv").unwrap().read_to_string(&mut csv).unwrap();
        assert!(csv.contains("Algebra-Bazovyy.pdf;Алгебра (Базовый);"));
        assert!(zip.by_name("Algebra-Bazovyy.pdf").is_ok());
        assert_eq!(fs::read(&source).unwrap(),b"original bytes");
        drop(zip);fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn safe_filename_uses_only_latin_digits_hyphens_underscores_and_extension_dot() {
        assert_eq!(clean_import_filename("Приказ №12 (новый)!.pdf"), "Prikaz-12-novyy.pdf");
        assert_eq!(clean_import_filename("отчёт_за-2026.pdf"), "otchyot_za-2026.pdf");
        assert_eq!(clean_import_filename("a.b@c#d?.PDF"), "a-b-c-d.PDF");
        assert_eq!(clean_import_filename("CON.txt"), "CON_.txt");
        assert_eq!(clean_import_filename("!!!.pdf"), "file.pdf");
        assert_eq!(clean_import_filename("a<>/|\\\\b.docx"), "a-b.docx");
        for raw in ["name%20.pdf","тест(документ).pdf","special+chars+=.doc",
            "name\\u{2603}\\u{1F600}.txt", "a.b@c#d?.PDF", "отчёт_за-2026.pdf"]
        {
            let filename = clean_import_filename(raw);
            let (stem, extension) = name_parts(&filename);
            assert!(stem.chars().all(|ch|ch.is_ascii_alphanumeric()||ch=='-'||ch=='_'), "{filename}");
            if let Some(ext) = extension {
                assert!(ext.chars().all(|ch|ch.is_ascii_alphanumeric()||ch=='-'||ch=='_'),"{filename}");
            }
            assert!(valid_filename(&filename),"{filename}");
        }
    }
    #[test]
    fn sanitizes_filename_in_export_zip_and_csv_preserving_russian_name() {
        let root = sandbox("safe_export");
        let template_path = root.join("component_import.csv");
        fs::write(&template_path, "Name;File;Extra\n").unwrap();
        let source = root.join("РП Алгебра (базовая)_2026 №1.pdf");
        fs::write(&source,b"original document").unwrap();
        let template = read_template(&template_path).unwrap();
        let mut rows = scan(&root,&root.join("app.exe"),&template).unwrap();
        assert_eq!(rows[0].fields[0],"РП Алгебра (базовая)_2026 №1");
        rows[0].fields[1] = "РП_Алгебра+1(новая)!№.pdf".to_owned();
        let archive = export(&root,&rows,&template).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
        let filename = "RP_Algebra-1-novaya.pdf";
        let mut csv = String::new();
        zip.by_name("component_import.csv").unwrap().read_to_string(&mut csv).unwrap();
        assert!(csv.contains(&format!("РП Алгебра (базовая)_2026 №1;{filename};")));
        assert_eq!(zip.by_name(filename).unwrap().size(),17);
        assert_eq!(fs::read(&source).unwrap(),b"original document");
        assert_eq!(rows[0].fields[1],"РП_Алгебра+1(новая)!№.pdf");
        drop(zip);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn saved_project_uses_relative_source_paths_and_survives_folder_move() {
        let original = sandbox("project_portable_original");
        let destination = sandbox("project_portable_new");
        let template_path = original.join("component_import.csv");
        fs::write(&template_path,"Name;File;Custom\n").unwrap();
        let t = read_template(&template_path).unwrap();
        let filename = "РП Математика (профиль).pdf";
        let source = original.join(filename);
        fs::write(&source,b"PDF").unwrap();
        let rows = scan(&original,&original.join("app.exe"),&t).unwrap();
        save_project(&original,&rows,&t).unwrap();
        let saved = fs::read_to_string(original.join(PROJECT_NAME)).unwrap();
        assert!(!saved.contains(&original.display().to_string()));
        fs::copy(&source,destination.join(filename)).unwrap();
        fs::copy(&template_path,destination.join("component_import.csv")).unwrap();
        fs::copy(original.join(PROJECT_NAME),destination.join(PROJECT_NAME)).unwrap();
        let project = load_project(&destination.join(PROJECT_NAME)).unwrap();
        let mut moved = restore_project_rows(project, &read_template(&destination.join("component_import.csv")).unwrap());
        relink_document_sources(&mut moved, &destination);
        assert_eq!(moved[0].source,destination.join(filename));
        assert!(moved[0].source.is_file());
        fs::remove_dir_all(original).unwrap();
        fs::remove_dir_all(destination).unwrap();
    }
    #[test]
    fn saving_project_twice_is_supported_on_windows_and_keeps_latest_edit() {
        let folder=sandbox("save_twice");
        let template_path=folder.join("component_import.csv");
        fs::write(&template_path,"Name;File;Custom\n").unwrap();
        let template=read_template(&template_path).unwrap();
        let source=folder.join("Документ.pdf");
        fs::write(&source,b"source").unwrap();
        let mut rows=scan(&folder,&folder.join("app.exe"),&template).unwrap();
        save_project(&folder,&rows,&template).unwrap();
        rows[0].fields[2] = "Изменённое значение".into();
        save_project(&folder,&rows,&template).unwrap();
        let restored=load_project(&folder.join(PROJECT_NAME)).unwrap();
        assert_eq!(restored.rows[0].fields[2],"Изменённое значение");
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn detects_any_csv_name_and_reports_multiple_templates() {
        let folder=sandbox("templates_any_name");
        fs::write(folder.join("Импорт новых документов.csv"),"File;Name;Category\n").unwrap();
        fs::write(folder.join("another_template.csv"),"Name;File\n").unwrap();
        fs::write(folder.join("notes.txt"),"Это просто заметки").unwrap();
        let found=template_candidates(&folder).unwrap();
        assert_eq!(found.len(),2);
        assert!(found.iter().any(|p|p.file_name().unwrap()=="Импорт новых документов.csv"));
        assert!(found.iter().any(|p|p.file_name().unwrap()=="another_template.csv"));
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn detects_valid_tsv_template_and_explains_excel_file() {
        let folder=sandbox("templates_ext");
        fs::write(folder.join("формат.tsv"),"Name\tFile\tOther\n").unwrap();
        assert_eq!(template_candidates(&folder).unwrap().len(),1);
        fs::remove_file(folder.join("формат.tsv")).unwrap();
        fs::write(folder.join("новый шаблон.xlsx"),b"not CSV").unwrap();
        let err=template_candidates(&folder).unwrap_err();
        assert!(err.contains("Excel"),"{err}");
        fs::remove_dir_all(folder).unwrap();
    }
    #[test]
    fn arbitrary_columns_and_csv_only_export_are_supported() {
        let root=sandbox("arbitrary_38");
        let headers=(0..38).map(|i|format!("Column_{i}")).collect::<Vec<_>>();
        let header_line=headers.join(";");
        let data=(0..38).map(|i|format!("Value_{i}")).collect::<Vec<_>>().join(";");
        let path=root.join("any_kind_of_template.csv");
        fs::write(&path,format!("{header_line}\r\n{data}\r\n")).unwrap();
        let template=read_template(&path).unwrap();
        assert_eq!(template.headers.len(),38);
        assert_eq!(template.title_column,None);
        assert_eq!(template.asset_column,None);
        let mut rows=rows_from_template(&template);
        assert!(!rows[0].included);
        rows[0].included=true;
        let archive=export(&root,&rows,&template).unwrap();
        let mut zip=zip::ZipArchive::new(File::open(archive).unwrap()).unwrap();
        assert_eq!(zip.len(),1);
        let mut actual=String::new();
        zip.by_name("component_import.csv").unwrap().read_to_string(&mut actual).unwrap();
        assert_eq!(actual,format!("{header_line}\r\n{data}\r\n"));
        drop(zip);
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn image_style_template_can_read_38_columns_without_required_file_name() {
        let root=sandbox("employee");
        let mut headers=vec!["Name".to_owned(),"Image".to_owned()];
        headers.extend((2..38).map(|i|format!("StaffInfo{i}")));
        let path=root.join("component_import (6).csv");
        let mut data=vec!["Иванов Иван".to_owned(),"ivanov.jpg".to_owned()];
        data.extend((2..38).map(|i|format!("value_{i}")));
        fs::write(&path,format!("{}\r\n{}\r\n",headers.join(";"),data.join(";"))).unwrap();
        let t=read_template(&path).unwrap();
        assert_eq!(t.headers.len(),38);
        assert_eq!(t.asset_column,Some(1));
        assert_eq!(t.title_column,Some(0));
        let samples=rows_from_template(&t);
        assert_eq!(samples.len(),1);
        assert!(!samples[0].included);
        assert_eq!(samples[0].fields[1],"ivanov.jpg");
        fs::write(root.join("Фото (новое).jpg"),b"JPG").unwrap();
        let found=scan(&root,&root.join("app.exe"),&t).unwrap();
        assert_eq!(found.len(),1);
        assert_eq!(found[0].fields[1],"Foto-novoe.jpg");
        fs::remove_dir_all(root).unwrap();
    }
    #[test]
    fn quotes_only_fields_that_require_it() {
        let template=TemplateSpec {
            path:PathBuf::from("a.csv"),headers:vec!["Name".into(),"File".into(),"Extra".into()],
            defaults:vec![String::new();3],delimiter:b';',example:false,
            title_column:Some(0),asset_column:Some(1),example_rows:vec![],
        };
        let document=Document {
            source:PathBuf::new(),included:true,
            fields:vec!["Название (скобки)".into(),"doc.pdf".into(),"содержит;разделитель".into()]
        };
        let csv=String::from_utf8(csv_bytes(&[document],&template).unwrap()).unwrap();
        assert!(csv.contains("Название (скобки);doc.pdf;\"содержит;разделитель\""));
    }
}

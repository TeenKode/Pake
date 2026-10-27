#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use eframe::egui::{self, Color32, RichText};
use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::fs::{self, File};
use std::io::{self, BufWriter, Read, Write};
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

fn quote_csv(value: &str) -> String {
    format!("\"{}\"", value.replace('"', "\"\""))
}
fn csv_bytes(rows: &[Document]) -> Vec<u8> {
    let mut csv = Vec::new();
    csv.extend_from_slice(HEADERS.join(";").as_bytes());
    csv.extend_from_slice(b"\r\n");
    for doc in rows.iter().filter(|row| row.included) {
        let row = doc.fields.iter().map(|field| quote_csv(field)).collect::<Vec<_>>().join(";");
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
    let file = File::create(archive)?;
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
    zip.finish().map_err(io::Error::other)?;
    Ok(())
}
fn export(folder: &Path, rows: &[Document]) -> Result<PathBuf, String> {
    validate_rows(rows)?;
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

struct ImporterApp {
    folder: PathBuf,
    rows: Vec<Document>,
    bulk_column: usize,
    bulk_value: String,
    only_checked: bool,
    filter: String,
    page: usize,
    active_row: Option<usize>,
    status: String,
    error: bool,
    confirm_rescan: bool,
    dirty: bool,
}

impl ImporterApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        cc.egui_ctx.set_visuals(egui::Visuals::light());
        cc.egui_ctx.style_mut(|style| {
            style.spacing.item_spacing = egui::vec2(8.0, 7.0);
            style.visuals.widgets.active.bg_fill = Color32::from_rgb(205, 230, 255);
            style.visuals.selection.bg_fill = Color32::from_rgb(60, 122, 194);
        });
        let exe = std::env::current_exe().unwrap_or_default();
        let folder = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut app = Self {
            folder, rows: Vec::new(), bulk_column: 5, bulk_value: String::new(),
            only_checked: false, filter: String::new(), page: 0,
            active_row: None, status: String::new(), error: false,
            confirm_rescan: false, dirty: false,
        };
        app.open_folder(app.folder.clone());
        app
    }
    fn info(&mut self, value: String) { self.status = value; self.error = false; }
    fn fail(&mut self, value: String) { self.status = value; self.error = true; }
    fn open_folder(&mut self, folder: PathBuf) {
        let project = folder.join(PROJECT_NAME);
        let rows = if project.is_file() {
            load_project(&project).map(|p| p.rows).or_else(|_| {
                scan(&folder, &std::env::current_exe().unwrap_or_default()).map_err(|e| e.to_string())
            })
        } else {
            scan(&folder, &std::env::current_exe().unwrap_or_default()).map_err(|e| e.to_string())
        };
        match rows {
            Ok(rows) => {
                self.folder = folder;
                self.rows = rows;
                self.active_row = None;
                self.filter.clear();
                self.page = 0;
                self.dirty = false;
                self.info(format!("Загружено файлов: {}. Можно редактировать любую ячейку.", self.rows.len()));
            }
            Err(error) => self.fail(format!("Ошибка чтения папки: {error}")),
        }
    }
    fn rescan(&mut self) {
        match scan(&self.folder, &std::env::current_exe().unwrap_or_default()) {
            Ok(rows) => {
                self.rows = rows;
                self.active_row = None;
                self.page = 0;
                self.dirty = true;
                self.info(format!("Список обновлён: {} файлов.", self.rows.len()));
            }
            Err(error) => self.fail(error.to_string()),
        }
    }
    fn persist(&mut self) {
        match save_project(&self.folder, &self.rows) {
            Ok(path) => {
                self.dirty = false;
                self.info(format!("Проект сохранён: {}", path.display()));
            }
            Err(error) => self.fail(format!("Ошибка сохранения проекта: {error}")),
        }
    }
    fn run_export(&mut self) {
        match export(&self.folder, &self.rows) {
            Ok(path) => {
                let _ = save_project(&self.folder, &self.rows);
                self.dirty = false;
                self.info(format!("Готово! Архив для импорта: {}", path.display()));
            }
            Err(error) => self.fail(error),
        }
    }
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.heading(RichText::new("Импорт документов").size(22.0));
            ui.label(RichText::new("v3.0").color(Color32::GRAY));
            ui.separator();
            if ui.button("Выбрать папку…").clicked() {
                if let Some(folder) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder() {
                    self.open_folder(folder);
                }
            }
            if ui.button("Обновить список").clicked() {
                if self.dirty { self.confirm_rescan = true; } else { self.rescan(); }
            }
            if ui.button("Сохранить проект").clicked() { self.persist(); }
            if ui.button(RichText::new("Создать CSV + ZIP").strong()).clicked() { self.run_export(); }
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new("Папка:").strong());
            ui.add(egui::Label::new(self.folder.to_string_lossy()).truncate());
        });
        if !self.status.is_empty() {
            let color = if self.error { Color32::from_rgb(180, 45, 45) } else { Color32::from_rgb(27, 116, 82) };
            ui.label(RichText::new(&self.status).color(color));
        }
    }
    fn bulk_controls(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Массовая правка").strong());
            egui::ComboBox::from_id_salt("bulk_column")
                .selected_text(format!("{} ({})", LABELS[self.bulk_column], HEADERS[self.bulk_column]))
                .width(205.0)
                .show_ui(ui, |ui| {
                    for idx in 0..14 {
                        ui.selectable_value(&mut self.bulk_column, idx, format!("{} · {}", LABELS[idx], HEADERS[idx]));
                    }
                });
            ui.add(egui::TextEdit::singleline(&mut self.bulk_value)
                .hint_text("Общее значение для всех строк")
                .desired_width(295.0));
            ui.checkbox(&mut self.only_checked, "Только отмеченные");
            if ui.button("Применить ко всем").clicked() {
                let amount = bulk_edit(&mut self.rows, self.bulk_column, &self.bulk_value, self.only_checked);
                self.dirty = amount != 0;
                self.info(format!("Обновлено {} строк в колонке «{}».", amount, LABELS[self.bulk_column]));
            }
            if ui.button("Очистить колонку").clicked() {
                let amount = bulk_edit(&mut self.rows, self.bulk_column, "", self.only_checked);
                self.dirty = amount != 0;
                self.info(format!("Очищено {} строк в колонке «{}».", amount, LABELS[self.bulk_column]));
            }
        });
        ui.horizontal_wrapped(|ui| {
            ui.label("Фильтр:");
            let changed = ui.add(egui::TextEdit::singleline(&mut self.filter)
                .hint_text("Поиск по любому полю…").desired_width(240.0)).changed();
            if changed { self.page = 0; }
            if ui.small_button("Все ✓").clicked() {
                for row in &mut self.rows { row.included = true; }
                self.dirty = true;
            }
            if ui.small_button("Снять ✓").clicked() {
                for row in &mut self.rows { row.included = false; }
                self.dirty = true;
            }
            ui.label(format!("Всего: {} · Для экспорта: {} · Изменения: {}",
                self.rows.len(), self.rows.iter().filter(|r| r.included).count(),
                if self.dirty { "не сохранены" } else { "сохранены" }));
        });
    }
    fn filtered_indices(&self) -> Vec<usize> {
        let query = self.filter.trim().to_lowercase();
        self.rows.iter().enumerate()
            .filter(|(_, row)| query.is_empty() || row.fields.iter().any(|s| s.to_lowercase().contains(&query))
                || row.source.to_string_lossy().to_lowercase().contains(&query))
            .map(|(i, _)| i).collect()
    }
    fn table(&mut self, ui: &mut egui::Ui) {
        let visible = self.filtered_indices();
        let page_count = visible.len().max(1).div_ceil(PAGE_SIZE);
        self.page = self.page.min(page_count-1);
        let start = self.page * PAGE_SIZE;
        let indices = &visible[start..visible.len().min(start+PAGE_SIZE)];
        ui.horizontal(|ui| {
            ui.label(RichText::new("Таблица документов").strong().size(16.0));
            ui.label("Двойной щелчок по тексту не нужен — редактирование прямо в ячейках.");
            ui.separator();
            ui.label(format!("Страница {} / {}", self.page+1, page_count));
            if ui.add_enabled(self.page>0, egui::Button::new("◀")).clicked() { self.page -= 1; }
            if ui.add_enabled(self.page+1<page_count, egui::Button::new("▶")).clicked() { self.page += 1; }
        });
        ui.separator();
        egui::ScrollArea::both().id_salt("document_table_scroll")
            .auto_shrink([false, false]).show(ui, |ui| {
            egui::Grid::new("editable_document_grid")
                .striped(true)
                .spacing([9.0, 7.0])
                .show(ui, |ui| {
                    ui.strong("Выбор");
                    ui.strong("№");
                    for (n, label) in LABELS.iter().enumerate() {
                        ui.add_sized([COLUMN_WIDTHS[n], 20.0],
                            egui::Label::new(RichText::new(format!("{}\n{}", label, HEADERS[n])).strong()));
                    }
                    ui.end_row();
                    for &idx in indices {
                        let row = &mut self.rows[idx];
                        if ui.checkbox(&mut row.included, "").changed() { self.dirty = true; }
                        if ui.selectable_label(self.active_row == Some(idx), (idx+1).to_string()).clicked() {
                            self.active_row = Some(idx);
                        }
                        for col in 0..14 {
                            let field = &mut row.fields[col];
                            let edit = egui::TextEdit::singleline(field)
                                .desired_width(COLUMN_WIDTHS[col])
                                .clip_text(true);
                            if ui.add(edit).changed() { self.dirty = true; }
                        }
                        ui.end_row();
                    }
                });
        });
    }
    fn row_editor(&mut self, ui: &mut egui::Ui) {
        ui.heading("Редактор строки");
        ui.label("Выбери номер строки в таблице.");
        ui.separator();
        let Some(index) = self.active_row else {
            ui.label("Номер строки слева от ячеек открывает удобное редактирование всех полей документа.");
            return;
        };
        if index >= self.rows.len() { self.active_row = None; return; }
        let doc = &mut self.rows[index];
        ui.label(RichText::new(format!("Документ №{}", index+1)).strong());
        ui.small(doc.source.file_name().unwrap_or_default().to_string_lossy());
        if ui.checkbox(&mut doc.included, "Добавить в импорт").changed() { self.dirty = true; }
        ui.separator();
        egui::ScrollArea::vertical().show(ui, |ui| {
            for col in 0..14 {
                ui.label(RichText::new(format!("{} · {}", LABELS[col], HEADERS[col])).strong());
                let response = if matches!(col, 4 | 8 | 9 | 13) {
                    ui.add(egui::TextEdit::multiline(&mut doc.fields[col]).desired_rows(3).desired_width(f32::INFINITY))
                } else {
                    ui.add(egui::TextEdit::singleline(&mut doc.fields[col]).desired_width(f32::INFINITY))
                };
                if response.changed() { self.dirty = true; }
                ui.add_space(5.0);
            }
        });
    }
}

impl eframe::App for ImporterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|input| input.modifiers.ctrl && input.key_pressed(egui::Key::S)) {
            self.persist();
        }
        egui::TopBottomPanel::top("main_toolbar").show(ctx, |ui| {
            self.toolbar(ui);
        });
        egui::TopBottomPanel::top("mass_edit_bar").show(ctx, |ui| {
            ui.add_space(3.0);
            self.bulk_controls(ui);
            ui.add_space(3.0);
        });
        egui::TopBottomPanel::bottom("hint_footer").show(ctx, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label("CSV: UTF-8 без BOM · Разделитель ; · Исходники не изменяются");
                ui.separator();
                ui.label("Ctrl+S — сохранить проект · ZIP содержит CSV и копии документов");
            });
        });
        egui::SidePanel::right("row_editor")
            .default_width(340.0)
            .min_width(260.0)
            .resizable(true)
            .show(ctx, |ui| self.row_editor(ui));
        egui::CentralPanel::default().show(ctx, |ui| self.table(ui));

        if self.confirm_rescan {
            let mut open = self.confirm_rescan;
            let mut yes = false;
            egui::Window::new("Сбросить изменения?")
                .collapsible(false).resizable(false)
                .open(&mut open)
                .show(ctx, |ui| {
                    ui.label("Повторное сканирование сбросит несохранённые правки.");
                    ui.horizontal(|ui| {
                        if ui.button("Да, обновить список").clicked() { yes = true; }
                        if ui.button("Отмена").clicked() { /* close via window next frame */ }
                    });
                });
            if yes { self.rescan(); open = false; }
            self.confirm_rescan = open;
        }
    }
}

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
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1420.0, 850.0])
            .with_min_inner_size([970.0, 590.0]),
        ..Default::default()
    };
    eframe::run_native("Импорт документов — редактирование CSV", options,
        Box::new(|cc| Ok(Box::new(ImporterApp::new(cc)))))
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

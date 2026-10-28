use super::*;
use egui::{Align, Button, Color32, CornerRadius, FontData, FontDefinitions, FontFamily, Frame, Margin, RichText, Stroke, TextEdit};
use egui_extras::{Column, TableBuilder};

const ACCENT: Color32 = Color32::from_rgb(64, 125, 229);
const SUCCESS: Color32 = Color32::from_rgb(53, 177, 132);
const WARNING: Color32 = Color32::from_rgb(236, 111, 113);
const BASIC: [usize; 4] = [0, 3, 5, 2];
const ALL: [usize; 14] = [0, 3, 5, 2, 1, 4, 6, 7, 8, 9, 10, 11, 12, 13];
const TABS: [&str; 3] = ["Основное", "Дополнительно", "Подпись"];

fn canvas(d: bool) -> Color32 { if d { Color32::from_rgb(20, 25, 34) } else { Color32::from_rgb(244, 247, 251) } }
fn surface(d: bool) -> Color32 { if d { Color32::from_rgb(29, 36, 48) } else { Color32::WHITE } }
fn border(d: bool) -> Color32 { if d { Color32::from_rgb(59, 70, 86) } else { Color32::from_rgb(222, 228, 237) } }
fn text_color(d: bool) -> Color32 { if d { Color32::from_rgb(233, 239, 248) } else { Color32::from_rgb(33, 45, 65) } }
fn subdued(d: bool) -> Color32 { if d { Color32::from_rgb(162, 176, 195) } else { Color32::from_rgb(112, 125, 143) } }
fn field_bg(d: bool) -> Color32 { if d { Color32::from_rgb(20, 27, 37) } else { Color32::from_rgb(249, 251, 254) } }

fn setup_fonts(ctx: &egui::Context) {
    // Windows Segoe UI supports Cyrillic and common UI punctuation.
    // The Symbol face provides a fallback for uncommon glyphs.
    let win = std::env::var_os("WINDIR").map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from(r"C:\Windows"));
    let dir = win.join("Fonts");
    let mut fonts = FontDefinitions::default();
    let mut new_faces = Vec::new();
    for (name, file) in [("windows-ui", "segoeui.ttf"), ("windows-symbol", "seguisym.ttf")] {
        if let Ok(bytes) = fs::read(dir.join(file)) {
            fonts.font_data.insert(name.to_owned(), FontData::from_owned(bytes).into());
            new_faces.push(name.to_owned());
        }
    }
    if !new_faces.is_empty() {
        for family in [FontFamily::Proportional, FontFamily::Monospace] {
            let entries = fonts.families.entry(family).or_default();
            for face in new_faces.iter().rev() {
                if !entries.iter().any(|e| e == face) { entries.insert(0, face.to_owned()); }
            }
        }
        ctx.set_fonts(fonts);
    }
}
fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut v = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    v.panel_fill = canvas(dark);
    v.window_fill = surface(dark);
    v.extreme_bg_color = field_bg(dark);
    v.faint_bg_color = if dark { Color32::from_rgb(37, 45, 59) } else { Color32::from_rgb(238, 243, 251) };
    v.selection.bg_fill = ACCENT.gamma_multiply(0.75);
    v.selection.stroke.color = Color32::WHITE;
    v.widgets.noninteractive.bg_fill = surface(dark);
    v.widgets.inactive.bg_fill = field_bg(dark);
    v.widgets.hovered.bg_fill = if dark { Color32::from_rgb(54, 68, 91) } else { Color32::from_rgb(229, 238, 252) };
    v.widgets.active.bg_fill = if dark { Color32::from_rgb(57, 88, 132) } else { Color32::from_rgb(209, 225, 251) };
    v.widgets.noninteractive.fg_stroke.color = text_color(dark);
    v.widgets.inactive.fg_stroke.color = text_color(dark);
    v.widgets.hovered.fg_stroke.color = text_color(dark);
    v.widgets.active.fg_stroke.color = text_color(dark);
    for w in [&mut v.widgets.noninteractive, &mut v.widgets.inactive, &mut v.widgets.hovered, &mut v.widgets.active] {
        w.bg_stroke = Stroke::new(1.0, border(dark));
        w.corner_radius = CornerRadius::same(5);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(5.0, 5.0);
        style.spacing.button_padding = egui::vec2(8.0, 4.0);
        style.spacing.interact_size.y = 25.0;
        style.spacing.indent = 12.0;
    });
}
fn button(name: &str, dark: bool) -> Button<'_> {
    Button::new(name).fill(surface(dark)).stroke(Stroke::new(1.0, border(dark)))
        .corner_radius(5)
}
fn primary(name: &str) -> Button<'_> {
    Button::new(RichText::new(name).strong().color(Color32::WHITE))
        .fill(ACCENT).stroke(Stroke::NONE).corner_radius(5)
}
fn title(ui: &mut egui::Ui, name: &str, d: bool) {
    ui.label(RichText::new(name).strong().color(text_color(d)));
}
fn label(ui: &mut egui::Ui, name: &str, d: bool) {
    ui.label(RichText::new(name).color(subdued(d)).size(11.0));
}

pub(super) struct ImporterApp {
    folder: PathBuf,
    rows: Vec<Document>,
    filter: String,
    only_included_filter: bool,
    active_row: Option<usize>,
    selected_tab: usize,
    dark: bool,
    full_columns: bool,
    show_editor: bool,
    show_bulk: bool,
    bulk_column: usize,
    bulk_value: String,
    bulk_checked: bool,
    undo_bulk: Option<Vec<[String; 14]>>,
    status: String,
    error: bool,
    dirty: bool,
    confirm_rescan: bool,
    pending_folder: Option<PathBuf>,
}
impl ImporterApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        setup_fonts(&cc.egui_ctx);
        apply_theme(&cc.egui_ctx, false);
        let exe = std::env::current_exe().unwrap_or_default();
        let folder = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut app = Self {
            folder, rows: Vec::new(), filter: String::new(), only_included_filter: false,
            active_row: None, selected_tab: 0, dark: false, full_columns: false,
            show_editor: true, show_bulk: false, bulk_column: 5, bulk_value: String::new(),
            bulk_checked: false, undo_bulk: None, status: String::new(), error: false,
            dirty: false, confirm_rescan: false, pending_folder: None,
        };
        app.open_folder(app.folder.clone());
        app
    }
    fn info(&mut self, s: impl Into<String>) { self.status = s.into(); self.error = false; }
    fn fail(&mut self, s: impl Into<String>) { self.status = s.into(); self.error = true; }
    fn open_folder(&mut self, folder: PathBuf) {
        let exe = std::env::current_exe().unwrap_or_default();
        let project = folder.join(PROJECT_NAME);
        let result = if project.is_file() {
            load_project(&project).map(|p| p.rows).or_else(|_| {
                scan(&folder, &exe).map_err(|e| e.to_string())
            })
        } else { scan(&folder, &exe).map_err(|e| e.to_string()) };
        match result {
            Ok(rows) => {
                self.folder = folder; self.rows = rows;
                self.filter.clear(); self.active_row = None;
                self.dirty = false; self.undo_bulk = None;
                self.info(format!("Загружено файлов: {}", self.rows.len()));
            }
            Err(e) => self.fail(e),
        }
    }
    fn rescan(&mut self) {
        let exe = std::env::current_exe().unwrap_or_default();
        match scan(&self.folder, &exe) {
            Ok(rows) => {
                self.rows = rows; self.active_row = None; self.undo_bulk = None;
                self.dirty = true; self.info("Список файлов обновлён.");
            }
            Err(e) => self.fail(e.to_string())
        }
    }
    fn choose_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder() {
            if self.dirty { self.pending_folder = Some(folder); }
            else { self.open_folder(folder); }
        }
    }
    fn save(&mut self) {
        match save_project(&self.folder, &self.rows) {
            Ok(_) => { self.dirty = false; self.info("Проект сохранён."); },
            Err(e) => self.fail(e),
        }
    }
    fn generate(&mut self) {
        match export(&self.folder, &self.rows) {
            Ok(path) => {
                if save_project(&self.folder, &self.rows).is_ok() { self.dirty = false; }
                self.info(format!("Создан архив: {}", path.display()));
            }
            Err(e) => self.fail(e)
        }
    }
    fn bulk_apply(&mut self, clear: bool) {
        self.undo_bulk = Some(self.rows.iter().map(|r| r.fields.clone()).collect());
        let value = if clear { "" } else { &self.bulk_value };
        let n = bulk_edit(&mut self.rows, self.bulk_column, value, self.bulk_checked);
        if n > 0 { self.dirty = true; }
        self.info(format!("Изменена колонка «{}»: {} строк.", LABELS[self.bulk_column], n));
    }
    fn undo(&mut self) {
        if let Some(values) = self.undo_bulk.take() {
            for (row, original) in self.rows.iter_mut().zip(values) { row.fields = original; }
            self.dirty = true; self.info("Массовое изменение отменено.");
        }
    }
    fn filtered(&self) -> Vec<usize> {
        let needle = self.filter.trim().to_lowercase();
        self.rows.iter().enumerate()
            .filter(|(_, row)| !self.only_included_filter || row.included)
            .filter(|(_, row)| needle.is_empty()
                || row.fields.iter().any(|field| field.to_lowercase().contains(&needle))
                || row.source.to_string_lossy().to_lowercase().contains(&needle))
            .map(|(i, _)| i).collect()
    }
    fn toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Импорт документов").size(18.0).strong().color(text_color(self.dark)));
            label(ui, "v5.1", self.dark);
            ui.separator();
            if ui.add(button("Папка", self.dark)).on_hover_text("Выбрать папку с документами").clicked() {
                self.choose_folder();
            }
            if ui.add(button("Обновить", self.dark)).on_hover_text("Повторно найти файлы").clicked() {
                if self.dirty { self.confirm_rescan = true; } else { self.rescan(); }
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.add(primary("Создать CSV + ZIP")).clicked() { self.generate(); }
                if ui.add(button("Сохранить", self.dark)).clicked() { self.save(); }
                if ui.add(button(if self.dark { "Светлая" } else { "Тёмная" }, self.dark))
                    .on_hover_text("Переключить тему").clicked() {
                    self.dark = !self.dark;
                    apply_theme(ui.ctx(), self.dark);
                }
            });
        });
        ui.horizontal(|ui| {
            ui.label(RichText::new(self.folder.display().to_string()).color(subdued(self.dark))
                .size(11.0));
            ui.separator();
            let checked = self.rows.iter().filter(|r| r.included).count();
            label(ui, &format!("Файлы: {} / {}", checked, self.rows.len()), self.dark);
            if self.dirty { ui.colored_label(WARNING, "Не сохранено"); }
        });
    }
    fn bulk_bar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            if ui.add(button(if self.show_bulk { "Скрыть массовую правку" } else { "Массовая правка" }, self.dark))
                .clicked() { self.show_bulk = !self.show_bulk; }
            if self.show_bulk {
                egui::ComboBox::from_id_salt("bulk_col_v5")
                    .selected_text(LABELS[self.bulk_column]).width(120.0)
                    .show_ui(ui, |ui| {
                        for idx in 0..14 {
                            ui.selectable_value(&mut self.bulk_column, idx, LABELS[idx]);
                        }
                    });
                ui.add(TextEdit::singleline(&mut self.bulk_value)
                    .hint_text("Значение").desired_width(210.0));
                ui.checkbox(&mut self.bulk_checked, "Отмеченные");
                if ui.add(primary("Заполнить")).clicked() { self.bulk_apply(false); }
                if ui.add(button("Очистить", self.dark)).clicked() { self.bulk_apply(true); }
                if ui.add_enabled(self.undo_bulk.is_some(), button("Отмена", self.dark)).clicked() { self.undo(); }
            }
        });
    }
    fn quick_filter(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.add(TextEdit::singleline(&mut self.filter)
                .hint_text("Поиск по документам").desired_width(240.0));
            ui.checkbox(&mut self.only_included_filter, "Отмеченные");
            if ui.add(button("Все", self.dark)).on_hover_text("Отметить все").clicked() {
                for row in &mut self.rows { row.included = true; }
                self.dirty = true;
            }
            if ui.add(button("Снять", self.dark)).on_hover_text("Снять все отметки").clicked() {
                for row in &mut self.rows { row.included = false; }
                self.dirty = true;
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.add(button(if self.show_editor { "Скрыть поля" } else { "Поля справа" }, self.dark)).clicked() {
                    self.show_editor = !self.show_editor;
                }
                if ui.add(button(if self.full_columns { "Кратко" } else { "Все столбцы" }, self.dark)).clicked() {
                    self.full_columns = !self.full_columns;
                }
            });
        });
    }
    fn table(&mut self, ui: &mut egui::Ui) {
        let visible = self.filtered();
        let cols: &[usize] = if self.full_columns { &ALL } else { &BASIC };
        let d = self.dark;
        let widths: [f32; 14] = [190.0, 90.0, 105.0, 225.0, 190.0, 120.0, 120.0, 120.0, 170.0,
            150.0, 120.0, 150.0, 150.0, 170.0];
        let minimum = (48.0 + cols.iter().map(|&i| widths[i]).sum::<f32>() + 60.0).max(ui.available_width());
        egui::ScrollArea::horizontal().id_salt("table_hscroll_v5").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_min_width(minimum);
            let mut builder = TableBuilder::new(ui)
                .id_salt(if self.full_columns { "resize_full_v5" } else { "resize_basic_v5" })
                .striped(true).resizable(true).auto_shrink([false, false])
                .column(Column::exact(28.0)).column(Column::exact(36.0));
            for &col in cols {
                builder = builder.column(Column::initial(widths[col]).at_least(65.0)
                    .at_most(1000.0).resizable(true).clip(true));
            }
            builder.header(25.0, |mut header| {
                header.col(|ui| { ui.label("В"); });
                header.col(|ui| { ui.label("#"); });
                for &col in cols {
                    header.col(|ui| {
                        ui.label(RichText::new(LABELS[col]).strong().color(text_color(d)))
                            .on_hover_text("Перетащите правую границу заголовка для изменения ширины");
                    });
                }
            }).body(|body| {
                body.rows(27.0, visible.len(), |mut row| {
                    let idx = visible[row.index()];
                    let doc = &mut self.rows[idx];
                    row.col(|ui| {
                        if ui.checkbox(&mut doc.included, "").changed() { self.dirty = true; }
                    });
                    row.col(|ui| {
                        if ui.selectable_label(self.active_row == Some(idx), (idx+1).to_string()).clicked() {
                            self.active_row = Some(idx);
                            self.show_editor = true;
                        }
                    });
                    for &col in cols {
                        row.col(|ui| {
                            if ui.add(TextEdit::singleline(&mut doc.fields[col])
                                .desired_width(f32::INFINITY).clip_text(true)
                                .margin(egui::vec2(3.0, 2.0))).changed() { self.dirty = true; }
                        });
                    }
                });
            });
        });
    }
    fn inspector(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            title(ui, "Поля документа", self.dark);
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("Закрыть").clicked() { self.show_editor = false; }
            });
        });
        let Some(idx) = self.active_row else {
            ui.add_space(6.0);
            label(ui, "Выберите номер строки в таблице.", self.dark);
            return;
        };
        if idx >= self.rows.len() { self.active_row = None; return; }
        let filename = self.rows[idx].source.file_name().unwrap_or_default().to_string_lossy().into_owned();
        ui.label(RichText::new(filename).color(subdued(self.dark)).size(11.0));
        ui.horizontal_wrapped(|ui| {
            for (i, name) in TABS.iter().enumerate() {
                if ui.selectable_label(self.selected_tab == i, *name).clicked() { self.selected_tab = i; }
            }
        });
        ui.separator();
        let fields: &[usize] = match self.selected_tab {
            0 => &[0, 3, 5, 2, 1],
            1 => &[4, 6, 7, 8, 9],
            _ => &[10, 11, 12, 13],
        };
        egui::ScrollArea::vertical().id_salt("fields_v5").show(ui, |ui| {
            for &col in fields {
                ui.horizontal(|ui| {
                    title(ui, LABELS[col], self.dark);
                    ui.label(RichText::new(HEADERS[col]).size(10.0).color(subdued(self.dark)));
                });
                if matches!(col, 4 | 8 | 9 | 13) {
                    if ui.add(TextEdit::multiline(&mut self.rows[idx].fields[col])
                        .desired_rows(2).desired_width(f32::INFINITY)).changed() { self.dirty = true; }
                } else {
                    if ui.add(TextEdit::singleline(&mut self.rows[idx].fields[col])
                        .desired_width(f32::INFINITY)).changed() { self.dirty = true; }
                }
                if col == 3 {
                    if ui.small_button("Создать имя из названия").clicked() {
                        let stem = self.rows[idx].fields[0].clone();
                        let ext = self.rows[idx].source.extension().and_then(|e| e.to_str()).unwrap_or("");
                        let name = if ext.is_empty() { stem } else { format!("{stem}.{ext}") };
                        self.rows[idx].fields[3] = transliterate_filename(&name);
                        self.dirty = true;
                    }
                    if !valid_filename(self.rows[idx].fields[3].trim()) {
                        ui.colored_label(WARNING, "Некорректное имя файла");
                    }
                }
                ui.add_space(2.0);
            }
        });
    }
    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let color = if self.error { WARNING } else { if self.dark { SUCCESS } else { Color32::from_rgb(19, 119, 91) } };
            if !self.status.is_empty() {
                ui.label(RichText::new(&self.status).color(color).size(11.0));
            } else {
                label(ui, "UTF-8 без BOM | CSV + ZIP", self.dark);
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                label(ui, "Ctrl+S: сохранить | Ctrl+E: экспорт", self.dark);
            });
        });
    }
    fn confirmation(&mut self, ctx: &egui::Context) {
        if self.confirm_rescan {
            egui::Window::new("Обновить список?").collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("Несохранённые правки будут потеряны.");
                ui.horizontal(|ui| {
                    if ui.button("Обновить").clicked() { self.confirm_rescan = false; self.rescan(); }
                    if ui.button("Отмена").clicked() { self.confirm_rescan = false; }
                });
            });
        }
        if let Some(folder) = self.pending_folder.clone() {
            egui::Window::new("Сохранить изменения?").collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0]).show(ctx, |ui| {
                ui.label("Перед открытием другой папки сохранить проект?");
                ui.horizontal(|ui| {
                    if ui.button("Сохранить и открыть").clicked() {
                        self.save();
                        if !self.error { self.open_folder(folder.clone()); self.pending_folder = None; }
                    }
                    if ui.button("Не сохранять").clicked() { self.open_folder(folder.clone()); self.pending_folder = None; }
                    if ui.button("Отмена").clicked() { self.pending_folder = None; }
                });
            });
        }
    }
}
impl eframe::App for ImporterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S)) { self.save(); }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::E)) { self.generate(); }
        egui::TopBottomPanel::top("compact_toolbar")
            .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::symmetric(12, 7)))
            .show(ctx, |ui| self.toolbar(ui));
        egui::TopBottomPanel::top("compact_bulk")
            .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::symmetric(12, 3)))
            .show(ctx, |ui| self.bulk_bar(ui));
        egui::TopBottomPanel::bottom("compact_status")
            .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::symmetric(12, 5)))
            .show(ctx, |ui| self.footer(ui));
        if self.show_editor {
            egui::SidePanel::right("compact_details").resizable(true)
                .default_width(295.0).min_width(230.0)
                .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::same(10)))
                .show(ctx, |ui| self.inspector(ui));
        }
        egui::CentralPanel::default()
            .frame(Frame::new().fill(canvas(self.dark)).inner_margin(Margin::symmetric(7, 7)))
            .show(ctx, |ui| {
                ui.vertical(|ui| {
                    self.quick_filter(ui);
                    ui.separator();
                    self.table(ui);
                });
            });
        self.confirmation(ctx);
    }
}
pub(super) fn launch() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1370.0, 790.0])
            .with_min_inner_size([860.0, 520.0]),
        ..Default::default()
    };
    eframe::run_native("Импорт документов", options,
        Box::new(|cc| Ok(Box::new(ImporterApp::new(cc)))))
}

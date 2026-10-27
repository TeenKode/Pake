use super::*;
use egui::{Align, Button, CornerRadius, Frame, Margin, Stroke, TextEdit};

const BLUE: Color32 = Color32::from_rgb(44, 100, 210);
const BLUE_HOVER: Color32 = Color32::from_rgb(31, 82, 185);
const INK: Color32 = Color32::from_rgb(30, 45, 67);
const MUTED: Color32 = Color32::from_rgb(108, 122, 145);
const BORDER: Color32 = Color32::from_rgb(222, 229, 240);
const SURFACE: Color32 = Color32::WHITE;
const CANVAS: Color32 = Color32::from_rgb(244, 247, 252);
const SOFT_BLUE: Color32 = Color32::from_rgb(231, 239, 253);
const GREEN: Color32 = Color32::from_rgb(17, 132, 99);
const SOFT_GREEN: Color32 = Color32::from_rgb(225, 247, 238);
const BAD: Color32 = Color32::from_rgb(185, 55, 70);

const SIMPLE_COLS: [usize; 6] = [0, 3, 5, 2, 1, 4];
const TABS: [&str; 3] = ["Основное", "Описание и даты", "Подпись"];

fn apply_theme(ctx: &egui::Context, dark: bool) {
    let mut visuals = if dark { egui::Visuals::dark() } else { egui::Visuals::light() };
    visuals.panel_fill = if dark { Color32::from_rgb(25, 31, 42) } else { CANVAS };
    visuals.window_fill = if dark { Color32::from_rgb(34, 43, 57) } else { SURFACE };
    visuals.selection.bg_fill = BLUE.gamma_multiply(0.8);
    visuals.selection.stroke.color = Color32::WHITE;
    visuals.widgets.inactive.corner_radius = CornerRadius::same(7);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(7);
    visuals.widgets.active.corner_radius = CornerRadius::same(7);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(7);
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0, if dark { Color32::from_gray(78) } else { BORDER });
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0, if dark { Color32::from_gray(78) } else { BORDER });
    ctx.set_visuals(visuals);
    ctx.style_mut(|style| {
        style.spacing.item_spacing = egui::vec2(9.0, 8.0);
        style.spacing.button_padding = egui::vec2(12.0, 8.0);
        style.spacing.indent = 17.0;
        style.spacing.interact_size.y = 30.0;
        style.visuals.window_corner_radius = CornerRadius::same(12);
        style.visuals.menu_corner_radius = CornerRadius::same(9);
    });
}
fn surface(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(34, 43, 57) } else { SURFACE }
}
fn foreground(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(230, 235, 247) } else { INK }
}
fn subtle(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(171, 184, 207) } else { MUTED }
}
fn outline(dark: bool) -> Color32 {
    if dark { Color32::from_rgb(71, 83, 102) } else { BORDER }
}
fn card(dark: bool) -> Frame {
    Frame::new().fill(surface(dark))
        .stroke(Stroke::new(1.0, outline(dark)))
        .corner_radius(CornerRadius::same(11))
        .inner_margin(Margin::same(14))
}
fn secondary_button(label: &str, dark: bool) -> Button<'_> {
    Button::new(label).fill(surface(dark))
        .stroke(Stroke::new(1.0, outline(dark)))
        .corner_radius(7)
}
fn primary_button(label: &str) -> Button<'_> {
    Button::new(RichText::new(label).strong().color(Color32::WHITE))
        .fill(BLUE).stroke(Stroke::NONE).corner_radius(7)
}
fn small_label(ui: &mut egui::Ui, text: &str, dark: bool) {
    ui.label(RichText::new(text).size(11.5).color(subtle(dark)));
}
fn field_label(ui: &mut egui::Ui, title: &str, technical: &str, dark: bool) {
    ui.horizontal(|ui| {
        ui.label(RichText::new(title).strong().color(foreground(dark)));
        ui.label(RichText::new(technical).size(11.0).color(subtle(dark)));
    });
}

pub(super) struct ImporterApp {
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
    dark: bool,
    show_all_columns: bool,
    show_bulk: bool,
    show_inspector: bool,
    editor_tab: usize,
    only_included_filter: bool,
    undo_bulk: Option<Vec<[String; 14]>>,
    pending_folder: Option<PathBuf>,
}

impl ImporterApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        apply_theme(&cc.egui_ctx, false);
        let exe = std::env::current_exe().unwrap_or_default();
        let folder = exe.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut app = Self {
            folder, rows: Vec::new(), bulk_column: 5, bulk_value: String::new(),
            only_checked: false, filter: String::new(), page: 0,
            active_row: None, status: String::new(), error: false,
            confirm_rescan: false, dirty: false, dark: false,
            show_all_columns: false, show_bulk: false, show_inspector: true,
            editor_tab: 0, only_included_filter: false, undo_bulk: None,
            pending_folder: None,
        };
        app.open_folder(app.folder.clone());
        app
    }
    fn info(&mut self, value: String) { self.status = value; self.error = false; }
    fn fail(&mut self, value: String) { self.status = value; self.error = true; }
    fn open_folder(&mut self, folder: PathBuf) {
        let project = folder.join(PROJECT_NAME);
        let exe = std::env::current_exe().unwrap_or_default();
        let rows = if project.is_file() {
            load_project(&project).map(|p| p.rows).or_else(|_| {
                scan(&folder, &exe).map_err(|e| e.to_string())
            })
        } else {
            scan(&folder, &exe).map_err(|e| e.to_string())
        };
        match rows {
            Ok(rows) => {
                self.folder = folder;
                self.rows = rows;
                self.active_row = None;
                self.filter.clear();
                self.page = 0;
                self.undo_bulk = None;
                self.dirty = false;
                self.info(format!("Загружено документов: {}. Проверьте данные перед экспортом.", self.rows.len()));
            }
            Err(error) => self.fail(format!("Ошибка чтения папки: {error}")),
        }
    }
    fn rescan(&mut self) {
        match scan(&self.folder, &std::env::current_exe().unwrap_or_default()) {
            Ok(rows) => {
                self.rows = rows;
                self.active_row = None;
                self.undo_bulk = None;
                self.page = 0;
                self.dirty = true;
                self.info(format!("Обнаружено документов: {}.", self.rows.len()));
            }
            Err(error) => self.fail(error.to_string()),
        }
    }
    fn persist(&mut self) {
        match save_project(&self.folder, &self.rows) {
            Ok(_) => {
                self.dirty = false;
                self.info("Проект сохранён. Можно продолжить позже.".to_owned());
            }
            Err(error) => self.fail(format!("Ошибка сохранения проекта: {error}")),
        }
    }
    fn run_export(&mut self) {
        match export(&self.folder, &self.rows) {
            Ok(path) => {
                let _ = save_project(&self.folder, &self.rows);
                self.dirty = false;
                self.info(format!("Архив создан: {}", path.display()));
            }
            Err(error) => self.fail(error),
        }
    }
    fn change_folder(&mut self) {
        if let Some(folder) = rfd::FileDialog::new().set_directory(&self.folder).pick_folder() {
            if self.dirty { self.pending_folder = Some(folder); }
            else { self.open_folder(folder); }
        }
    }
    fn filtered_indices(&self) -> Vec<usize> {
        let search = self.filter.trim().to_lowercase();
        self.rows.iter().enumerate()
            .filter(|(_, row)| !self.only_included_filter || row.included)
            .filter(|(_, row)| search.is_empty()
                || row.fields.iter().any(|s| s.to_lowercase().contains(&search))
                || row.source.to_string_lossy().to_lowercase().contains(&search))
            .map(|(idx, _)| idx).collect()
    }
    fn snapshot_bulk(&mut self) {
        self.undo_bulk = Some(self.rows.iter().map(|row| row.fields.clone()).collect());
    }
    fn apply_bulk(&mut self, clear: bool) {
        self.snapshot_bulk();
        let value = if clear { "" } else { &self.bulk_value };
        let modified = bulk_edit(&mut self.rows, self.bulk_column, value, self.only_checked);
        if modified > 0 { self.dirty = true; }
        self.info(format!(
            "Колонка «{}»: изменено {} строк. Действие можно отменить.",
            LABELS[self.bulk_column], modified
        ));
    }
    fn restore_bulk(&mut self) {
        if let Some(previous) = self.undo_bulk.take() {
            for (row, fields) in self.rows.iter_mut().zip(previous) { row.fields = fields; }
            self.dirty = true;
            self.info("Последняя массовая правка отменена.".into());
        }
    }

    fn header(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("ДОКУМЕНТЫ / ИМПОРТ").size(11.0).color(BLUE).strong());
                ui.label(RichText::new("Подготовка документов").size(26.0)
                    .color(foreground(self.dark)).strong());
                ui.label(RichText::new("Настройте поля, затем выгрузите архив для системы.")
                    .size(12.0).color(subtle(self.dark)));
            });
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.add(primary_button("  Создать CSV + ZIP  ")).clicked() { self.run_export(); }
                if ui.add(secondary_button("Сохранить проект", self.dark)).clicked() { self.persist(); }
                if ui.add(secondary_button("Выбрать папку", self.dark)).clicked() { self.change_folder(); }
                let next_theme = if self.dark { "Светлая тема" } else { "Тёмная тема" };
                if ui.add(secondary_button(next_theme, self.dark)).clicked() {
                    self.dark = !self.dark;
                    apply_theme(ui.ctx(), self.dark);
                }
            });
        });
        ui.add_space(8.0);
        card(self.dark).show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                ui.label(RichText::new("ПАПКА").strong().size(11.0).color(subtle(self.dark)));
                ui.label(RichText::new(self.folder.display().to_string()).color(foreground(self.dark)));
                ui.separator();
                if ui.small_button("Сканировать снова").clicked() {
                    if self.dirty { self.confirm_rescan = true; } else { self.rescan(); }
                }
                if self.dirty {
                    ui.label(RichText::new("● Несохранённые изменения").color(Color32::from_rgb(184, 114, 25)));
                } else {
                    ui.label(RichText::new("● Изменения сохранены").color(GREEN));
                }
            });
        });
    }
    fn overview(&mut self, ui: &mut egui::Ui) {
        let total = self.rows.len();
        let included = self.rows.iter().filter(|r| r.included).count();
        let missing_file = self.rows.iter().filter(|r| r.included && (!valid_filename(r.fields[3].trim()) || !r.source.is_file())).count();
        let valid = validate_rows(&self.rows).is_ok();
        ui.columns(3, |cols| {
            for (n, (label, number, tip)) in [
                ("НАЙДЕНО", total, "документов в выбранной папке"),
                ("К ИМПОРТУ", included, "отмечено для экспорта"),
                ("ПРОВЕРКА", if valid { 0 } else { missing_file.max(1) }, if valid { "ошибок не найдено" } else { "есть замечания к файлам" }),
            ].iter().enumerate() {
                card(self.dark).show(&mut cols[n], |ui| {
                    ui.label(RichText::new(*label).size(10.5).strong().color(subtle(self.dark)));
                    ui.horizontal(|ui| {
                        let tint = if n == 2 && !valid { BAD } else if n == 1 { BLUE } else { foreground(self.dark) };
                        ui.label(RichText::new(number.to_string()).size(26.0).strong().color(tint));
                        ui.label(RichText::new(*tip).size(11.0).color(subtle(self.dark)));
                    });
                });
            }
        });
    }
    fn bulk_panel(&mut self, ui: &mut egui::Ui) {
        card(self.dark).show(ui, |ui| {
            ui.horizontal(|ui| {
                if ui.add(secondary_button(if self.show_bulk { "▾" } else { "▸" }, self.dark)).clicked() {
                    self.show_bulk = !self.show_bulk;
                }
                ui.vertical(|ui| {
                    ui.label(RichText::new("Массовое заполнение").size(15.5).strong().color(foreground(self.dark)));
                    small_label(ui, "Одно значение сразу для всех документов или только для отмеченных.", self.dark);
                });
                ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                    if ui.add_enabled(self.undo_bulk.is_some(), secondary_button("Отменить правку", self.dark)).clicked() {
                        self.restore_bulk();
                    }
                });
            });
            if self.show_bulk {
                ui.add_space(8.0);
                ui.separator();
                ui.add_space(4.0);
                ui.horizontal_wrapped(|ui| {
                    ui.label("Колонка:");
                    egui::ComboBox::from_id_salt("bulk_column_new")
                        .selected_text(LABELS[self.bulk_column]).width(195.0)
                        .show_ui(ui, |ui| {
                            for idx in 0..14 {
                                ui.selectable_value(&mut self.bulk_column, idx,
                                    format!("{}  ·  {}", LABELS[idx], HEADERS[idx]));
                            }
                        });
                    ui.label("Значение:");
                    ui.add(TextEdit::singleline(&mut self.bulk_value)
                        .hint_text("Например: Локальные акты").desired_width(275.0));
                    ui.checkbox(&mut self.only_checked, "Только отмеченные");
                });
                ui.horizontal(|ui| {
                    if ui.add(primary_button("Применить ко всем")).clicked() { self.apply_bulk(false); }
                    if ui.add(secondary_button("Очистить колонку", self.dark)).clicked() { self.apply_bulk(true); }
                    ui.label(RichText::new(if self.only_checked {
                        "Применяется только к документам с галочкой"
                    } else { "Применяется ко всем строкам таблицы" }).size(11.0).color(subtle(self.dark)));
                });
                if self.bulk_column == 3 {
                    ui.colored_label(BAD, "Внимание: одинаковые имена в колонке «Файл» не допускаются при экспорте.");
                }
            }
        });
    }
    fn search_toolbar(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("Документы").size(18.0).strong().color(foreground(self.dark)));
            ui.separator();
            if ui.add(TextEdit::singleline(&mut self.filter)
                .hint_text("Поиск по названию или полям...")
                .desired_width(285.0)).changed() { self.page = 0; }
            if ui.checkbox(&mut self.only_included_filter, "Только отмеченные").changed() { self.page = 0; }
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.add(secondary_button(
                    if self.show_all_columns { "Основные поля" } else { "Все 14 колонок" }, self.dark
                )).clicked() { self.show_all_columns = !self.show_all_columns; }
                if ui.add(secondary_button(
                    if self.show_inspector { "Скрыть редактор" } else { "Показать редактор" }, self.dark
                )).clicked() { self.show_inspector = !self.show_inspector; }
            });
        });
        ui.horizontal_wrapped(|ui| {
            if ui.small_button("Отметить все").clicked() {
                for row in &mut self.rows { row.included = true; }
                self.dirty = true;
            }
            if ui.small_button("Снять все").clicked() {
                for row in &mut self.rows { row.included = false; }
                self.dirty = true;
            }
            ui.label(RichText::new("Нажмите номер строки, чтобы открыть полный редактор справа.")
                .size(11.0).color(subtle(self.dark)));
        });
    }
    fn document_table(&mut self, ui: &mut egui::Ui) {
        let visible = self.filtered_indices();
        let count = visible.len().max(1).div_ceil(PAGE_SIZE);
        self.page = self.page.min(count - 1);
        let begin = self.page * PAGE_SIZE;
        let indices = &visible[begin..visible.len().min(begin + PAGE_SIZE)];
        let columns: &[usize] = if self.show_all_columns {
            &[0, 3, 5, 2, 1, 4, 6, 7, 8, 9, 10, 11, 12, 13]
        } else { &SIMPLE_COLS };

        ui.horizontal(|ui| {
            ui.label(RichText::new(format!("Показано: {} из {}", visible.len(), self.rows.len()))
                .size(11.0).color(subtle(self.dark)));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.add_enabled(self.page + 1 < count, secondary_button("Следующая ›", self.dark)).clicked() { self.page += 1; }
                ui.label(format!("{} / {}", self.page + 1, count));
                if ui.add_enabled(self.page > 0, secondary_button("‹ Назад", self.dark)).clicked() { self.page -= 1; }
            });
        });
        ui.add_space(5.0);
        if visible.is_empty() {
            card(self.dark).show(ui, |ui| {
                ui.vertical_centered(|ui| {
                    ui.add_space(25.0);
                    ui.heading("Документы не найдены");
                    ui.label("Смените папку или очистите строку поиска.");
                    if ui.button("Сбросить фильтры").clicked() {
                        self.filter.clear();
                        self.only_included_filter = false;
                    }
                    ui.add_space(25.0);
                });
            });
            return;
        }
        let d = self.dark;
        let highlight = if d { Color32::from_rgb(43, 66, 103) } else { SOFT_BLUE };
        let columns_width = columns.iter().map(|&c| COLUMN_WIDTHS[c].max(115.0)).sum::<f32>();
        egui::ScrollArea::both().id_salt("documents_grid_v4").auto_shrink([false, false]).show(ui, |ui| {
            ui.set_min_width((columns_width + 115.0).max(ui.available_width()));
            egui::Grid::new("documents_cells_v4").striped(true)
                .spacing([10.0, 9.0]).show(ui, |ui| {
                    ui.add_sized([54.0, 25.0], egui::Label::new(RichText::new("Вкл.").strong().color(subtle(d))));
                    ui.add_sized([44.0, 25.0], egui::Label::new(RichText::new("№").strong().color(subtle(d))));
                    for &col in columns {
                        ui.add_sized([COLUMN_WIDTHS[col], 25.0],
                            egui::Label::new(RichText::new(LABELS[col]).strong().color(foreground(d))));
                    }
                    ui.end_row();
                    for &index in indices {
                        let row = &mut self.rows[index];
                        if ui.checkbox(&mut row.included, "").changed() { self.dirty = true; }
                        let selected = self.active_row == Some(index);
                        if ui.add_sized([43.0, 30.0], Button::new(RichText::new((index+1).to_string()).strong())
                            .fill(if selected { highlight } else { surface(d) })
                            .corner_radius(6))
                            .clicked() {
                                self.active_row = Some(index);
                                self.show_inspector = true;
                        }
                        for &col in columns {
                            let response = ui.add_sized([COLUMN_WIDTHS[col], 30.0],
                                TextEdit::singleline(&mut row.fields[col]).clip_text(true));
                            if response.changed() { self.dirty = true; }
                            response.on_hover_text(format!("{} · {}", LABELS[col], HEADERS[col]));
                        }
                        ui.end_row();
                    }
                });
        });
    }
    fn row_editor(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(RichText::new("Карточка документа").size(18.0).strong().color(foreground(self.dark)));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                if ui.small_button("✕").clicked() { self.show_inspector = false; }
            });
        });
        ui.add_space(4.0);
        let Some(index) = self.active_row else {
            card(self.dark).show(ui, |ui| {
                ui.add_space(15.0);
                ui.vertical_centered(|ui| {
                    ui.label(RichText::new("Выберите документ").size(17.0).strong());
                    ui.label(RichText::new("Нажмите номер строки в таблице, чтобы редактировать все поля без горизонтальной прокрутки.")
                        .color(subtle(self.dark)));
                });
                ui.add_space(15.0);
            });
            return;
        };
        if index >= self.rows.len() { self.active_row = None; return; }
        {
            let source_name = self.rows[index].source.file_name()
                .unwrap_or_default().to_string_lossy().into_owned();
            ui.label(RichText::new(format!("Строка {} · {}", index+1, source_name))
                .size(12.0).color(subtle(self.dark)));
        }
        ui.add_space(5.0);
        ui.horizontal_wrapped(|ui| {
            for (i, tab) in TABS.iter().enumerate() {
                if ui.selectable_label(self.editor_tab == i, *tab).clicked() { self.editor_tab = i; }
            }
        });
        ui.separator();
        let tab_columns: &[usize] = match self.editor_tab {
            0 => &[0, 3, 5, 2, 1],
            1 => &[4, 6, 7, 8, 9],
            _ => &[10, 11, 12, 13],
        };
        egui::ScrollArea::vertical().id_salt("inspector_scroll_v4").show(ui, |ui| {
            if ui.checkbox(&mut self.rows[index].included, "Включить в экспорт").changed() { self.dirty = true; }
            ui.add_space(8.0);
            for &col in tab_columns {
                field_label(ui, LABELS[col], HEADERS[col], self.dark);
                let response = if matches!(col, 4 | 8 | 9 | 13) {
                    ui.add(TextEdit::multiline(&mut self.rows[index].fields[col])
                        .desired_rows(4).desired_width(f32::INFINITY))
                } else {
                    ui.add(TextEdit::singleline(&mut self.rows[index].fields[col])
                        .desired_width(f32::INFINITY))
                };
                if response.changed() { self.dirty = true; }
                if col == 3 && !valid_filename(self.rows[index].fields[3].trim()) {
                    ui.colored_label(BAD, "Некорректное имя. Проверьте символы.");
                }
                ui.add_space(7.0);
            }
        });
    }
    fn footer(&mut self, ui: &mut egui::Ui) {
        ui.horizontal_wrapped(|ui| {
            ui.label(RichText::new("UTF-8 без BOM").color(GREEN).strong());
            ui.separator();
            ui.label(RichText::new("CSV · разделитель ;").color(subtle(self.dark)));
            ui.separator();
            ui.label(RichText::new("Исходные файлы остаются без изменений").color(subtle(self.dark)));
            ui.with_layout(egui::Layout::right_to_left(Align::Center), |ui| {
                ui.label(RichText::new("Ctrl+S — сохранить  ·  Ctrl+E — экспорт")
                    .color(subtle(self.dark)));
            });
        });
        if !self.status.is_empty() {
            ui.add_space(3.0);
            let tint = if self.error { BAD } else { GREEN };
            let message = self.status.clone();
            Frame::new()
                .fill(if self.error { Color32::from_rgb(252, 237, 238) } else { SOFT_GREEN })
                .inner_margin(Margin::symmetric(10, 7))
                .corner_radius(6)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(if self.error { "Ошибка" } else { "Готово" }).strong().color(tint));
                        ui.label(RichText::new(message).color(if self.error { BAD } else { INK }));
                    });
                });
        }
    }
    fn confirmation(&mut self, ctx: &egui::Context) {
        if self.confirm_rescan {
            egui::Window::new("Обновить список документов?")
                .collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Несохранённые правки будут потеряны при новом сканировании.");
                    ui.add_space(8.0);
                    ui.horizontal(|ui| {
                        if ui.add(primary_button("Обновить и сбросить")).clicked() {
                            self.confirm_rescan = false;
                            self.rescan();
                        }
                        if ui.button("Отмена").clicked() { self.confirm_rescan = false; }
                    });
                });
        }
        if let Some(folder) = self.pending_folder.clone() {
            egui::Window::new("Перейти к другой папке?")
                .collapsible(false).resizable(false).anchor(egui::Align2::CENTER_CENTER, [0.0, 0.0])
                .show(ctx, |ui| {
                    ui.label("Вы изменили документы. Сохранить текущий проект?");
                    ui.horizontal(|ui| {
                        if ui.add(primary_button("Сохранить и открыть")).clicked() {
                            self.persist();
                            if !self.error { self.open_folder(folder.clone()); self.pending_folder = None; }
                        }
                        if ui.button("Не сохранять").clicked() {
                            self.open_folder(folder.clone());
                            self.pending_folder = None;
                        }
                        if ui.button("Отмена").clicked() { self.pending_folder = None; }
                    });
                });
        }
    }
}
impl eframe::App for ImporterApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::S)) { self.persist(); }
        if ctx.input(|i| i.modifiers.ctrl && i.key_pressed(egui::Key::E)) { self.run_export(); }
        egui::TopBottomPanel::top("header_v4")
            .frame(Frame::new().fill(surface(self.dark))
                .inner_margin(Margin::symmetric(21, 16)))
            .show(ctx, |ui| self.header(ui));
        egui::TopBottomPanel::top("dashboard_v4")
            .frame(Frame::new().fill(if self.dark { Color32::from_rgb(25, 31, 42) } else { CANVAS })
                .inner_margin(Margin::symmetric(20, 11)))
            .show(ctx, |ui| {
                self.overview(ui);
                ui.add_space(10.0);
                self.bulk_panel(ui);
            });
        egui::TopBottomPanel::bottom("footer_v4")
            .frame(Frame::new().fill(surface(self.dark))
                .inner_margin(Margin::symmetric(20, 10)))
            .show(ctx, |ui| self.footer(ui));
        if self.show_inspector {
            egui::SidePanel::right("inspector_v4")
                .resizable(true).default_width(375.0).min_width(310.0)
                .frame(Frame::new().fill(surface(self.dark))
                    .inner_margin(Margin::symmetric(17, 15)))
                .show(ctx, |ui| self.row_editor(ui));
        }
        egui::CentralPanel::default().frame(
            Frame::new().fill(if self.dark { Color32::from_rgb(25, 31, 42) } else { CANVAS })
                .inner_margin(Margin::symmetric(20, 14))
        ).show(ctx, |ui| {
            card(self.dark).show(ui, |ui| {
                self.search_toolbar(ui);
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(9.0);
                self.document_table(ui);
            });
        });
        self.confirmation(ctx);
    }
}

pub(super) fn launch() -> eframe::Result {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1550.0, 890.0])
            .with_min_inner_size([1050.0, 630.0]),
        ..Default::default()
    };
    eframe::run_native("Импорт документов — рабочая область", options,
        Box::new(|cc| Ok(Box::new(ImporterApp::new(cc)))))
}

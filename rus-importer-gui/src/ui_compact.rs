use super::*;
use egui::{Align,Button,Color32,CornerRadius,FontData,FontDefinitions,FontFamily,Frame,Margin,RichText,Stroke,TextEdit};
use egui_extras::{Column,TableBuilder};

const BLUE:Color32=Color32::from_rgb(64,125,229);
const GOOD:Color32=Color32::from_rgb(60,190,143);
const BAD:Color32=Color32::from_rgb(236,111,113);
fn canvas(d:bool)->Color32{if d{Color32::from_rgb(20,25,34)}else{Color32::from_rgb(244,247,251)}}
fn surface(d:bool)->Color32{if d{Color32::from_rgb(29,36,48)}else{Color32::WHITE}}
fn border(d:bool)->Color32{if d{Color32::from_rgb(59,70,86)}else{Color32::from_rgb(222,228,237)}}
fn ink(d:bool)->Color32{if d{Color32::from_rgb(233,239,248)}else{Color32::from_rgb(33,45,65)}}
fn dim(d:bool)->Color32{if d{Color32::from_rgb(162,176,195)}else{Color32::from_rgb(112,125,143)}}
fn edit_bg(d:bool)->Color32{if d{Color32::from_rgb(20,27,37)}else{Color32::from_rgb(249,251,254)}}
fn button(s:&str,d:bool)->Button<'_>{
    Button::new(s).fill(surface(d)).stroke(Stroke::new(1.0,border(d))).corner_radius(5)
}
fn action(s:&str)->Button<'_>{
    Button::new(RichText::new(s).strong().color(Color32::WHITE))
        .fill(BLUE).stroke(Stroke::NONE).corner_radius(5)
}
fn localized(header:&str)->&str{
    match header.to_lowercase().as_str(){
        "name"=>"Название","file"=>"Файл","number"=>"Номер",
        "regulatory"=>"Нормативный","description"=>"Описание",
        "category"=>"Категория","datecreated"=>"Дата создания",
        "dateupdated"=>"Дата изменения","hypertext"=>"Гипертекст",
        "graphic"=>"Графика","dateeds"=>"Дата ЭП",
        "fioeds"=>"ФИО подписанта","positioneds"=>"Должность",
        "eds"=>"Электронная подпись",_=>header
    }
}
fn initial_width(name:&str)->f32{
    match name.to_lowercase().as_str() {
        "name"|"file"=>210.0,
        "description"|"category"|"hypertext"|"graphic"=>160.0,
        "regulatory"=>110.0,
        _=>135.0,
    }
}
fn apply_theme(ctx:&egui::Context,dark:bool){
    let mut v=if dark{egui::Visuals::dark()}else{egui::Visuals::light()};
    v.panel_fill=canvas(dark);
    v.window_fill=surface(dark);
    v.extreme_bg_color=edit_bg(dark);
    v.faint_bg_color=if dark{Color32::from_rgb(37,45,59)}else{Color32::from_rgb(238,243,251)};
    v.selection.bg_fill=BLUE.gamma_multiply(0.75);
    v.selection.stroke.color=Color32::WHITE;
    v.widgets.noninteractive.bg_fill=surface(dark);
    v.widgets.inactive.bg_fill=edit_bg(dark);
    v.widgets.hovered.bg_fill=if dark{Color32::from_rgb(54,68,91)}else{Color32::from_rgb(229,238,252)};
    v.widgets.active.bg_fill=if dark{Color32::from_rgb(57,88,132)}else{Color32::from_rgb(209,225,251)};
    for widget in [&mut v.widgets.noninteractive,&mut v.widgets.inactive,&mut v.widgets.hovered,&mut v.widgets.active]{
        widget.bg_stroke=Stroke::new(1.0,border(dark));
        widget.fg_stroke.color=ink(dark);
        widget.corner_radius=CornerRadius::same(5);
    }
    ctx.set_visuals(v);
    ctx.style_mut(|s|{
        s.spacing.item_spacing=egui::vec2(5.0,5.0);
        s.spacing.button_padding=egui::vec2(8.0,4.0);
        s.spacing.interact_size.y=25.0;
        s.spacing.indent=12.0;
    });
}
fn setup_fonts(ctx:&egui::Context){
    let win=std::env::var_os("WINDIR").map(PathBuf::from)
        .unwrap_or_else(||PathBuf::from(r"C:\Windows"));
    let fonts_dir=win.join("Fonts");
    let mut fonts=FontDefinitions::default();
    let mut loaded=Vec::new();
    for (name,file) in [("windows-ui","segoeui.ttf"),("windows-symbol","seguisym.ttf")]{
        if let Ok(bytes)=fs::read(fonts_dir.join(file)){
            fonts.font_data.insert(name.into(),FontData::from_owned(bytes).into());
            loaded.push(name.to_owned());
        }
    }
    if !loaded.is_empty(){
        for family in [FontFamily::Proportional,FontFamily::Monospace]{
            let values=fonts.families.entry(family).or_default();
            for font in loaded.iter().rev(){values.insert(0,font.clone());}
        }
        ctx.set_fonts(fonts);
    }
}
fn prime_columns(t:&TemplateSpec)->Vec<usize>{
    let mut indices=Vec::new();
    for idx in [t.title_column,t.asset_column].into_iter().flatten(){
        if !indices.contains(&idx){indices.push(idx);}
    }
    for i in 0..t.headers.len(){
        if indices.len()>=5{break;}
        if !indices.contains(&i){indices.push(i);}
    }
    indices
}
pub(super) struct ImporterApp {
    folder:PathBuf,
    template:Option<TemplateSpec>,
    rows:Vec<Document>,
    filter:String,
    only_included:bool,
    active_row:Option<usize>,
    dark:bool,
    full_columns:bool,
    show_editor:bool,
    show_bulk:bool,
    bulk_column:usize,
    bulk_value:String,
    bulk_checked:bool,
    undo_bulk:Option<Vec<Vec<String>>>,
    status:String,
    error:bool,
    dirty:bool,
    confirm_rescan:bool,
    pending_folder:Option<PathBuf>,
    last_export:Option<PathBuf>,
    template_options:Vec<PathBuf>,
    pending_project:Option<Project>,
    show_mapping:bool,
}
impl ImporterApp {
    fn new(cc:&eframe::CreationContext<'_>)->Self{
        setup_fonts(&cc.egui_ctx);apply_theme(&cc.egui_ctx,false);
        let exe=std::env::current_exe().unwrap_or_default();
        let folder=exe.parent().unwrap_or(Path::new(".")).to_path_buf();
        let mut app=Self {
            folder,template:None,rows:Vec::new(),filter:String::new(),only_included:false,
            active_row:None,dark:false,full_columns:false,show_editor:false,
            show_bulk:false,bulk_column:0,bulk_value:String::new(),bulk_checked:false,
            undo_bulk:None,status:String::new(),error:false,dirty:false,
            confirm_rescan:false,pending_folder:None,last_export:None,
            template_options:Vec::new(),pending_project:None,show_mapping:false,
        };
        let folder=app.folder.clone();
        app.open_folder(folder);
        app
    }
    fn info(&mut self,s:impl Into<String>){self.status=s.into();self.error=false;}
    fn fail(&mut self,s:impl Into<String>){self.status=s.into();self.error=true;}
    fn open_folder(&mut self,folder:PathBuf){
        self.folder=folder.clone();self.template=None;self.rows.clear();self.active_row=None;
        self.undo_bulk=None;self.dirty=false;self.last_export=None;
        self.template_options.clear();self.pending_project=None;
        let project_path=folder.join(PROJECT_NAME);
        let project=if project_path.is_file() || folder.join(format!("{PROJECT_NAME}.bak")).is_file(){
            match load_project(&project_path){
                Ok(project)=>Some(project),
                Err(e)=>{self.fail(format!("Не удалось открыть сохранённый проект: {e}"));return;}
            }
        }else{None};
        let options=match template_candidates(&folder){
            Ok(options)=>options,
            Err(err)=>{self.fail(err);return;}
        };
        let valid:Vec<PathBuf>=options.iter().filter(|p|read_template(p).is_ok()).cloned().collect();
        let options=if valid.is_empty(){options}else{valid};
        if options.len()>1 {
            self.template_options=options;
            self.pending_project=project;
            self.info("Найдено несколько CSV-шаблонов. Выберите нужный из списка.");
            return;
        }
        self.open_template_path(options[0].clone(),project);
    }
    fn open_template_path(&mut self,path:PathBuf,project:Option<Project>){
        let mut new=match read_template(&path){Ok(t)=>t,Err(e)=>{self.fail(e);return;}};
        if let Some(project)=&project{apply_project_mapping(project,&mut new);}
        self.template_options.clear();self.pending_project=None;
        let executable=std::env::current_exe().unwrap_or_default();
        let rows=match project {
            Some(p)=>{
                let mut loaded=restore_project_rows(p,&new);
                relink_document_sources(&mut loaded,&self.folder);
                if loaded.is_empty(){
                    loaded=rows_from_template(&new);
                    let scanned=match scan(&self.folder,&executable,&new){
                        Ok(rows)=>rows,Err(e)=>{self.fail(format!("Ошибка сканирования: {e}"));return;}
                    };
                    loaded.extend(scanned);
                }
                loaded
            }
            None=>{
                let mut loaded=rows_from_template(&new);
                let scanned=match scan(&self.folder,&executable,&new){
                    Ok(rows)=>rows,Err(e)=>{self.fail(format!("Ошибка сканирования: {e}"));return;}
                };
                loaded.extend(scanned);
                loaded
            },
        };
        self.rows=rows;self.template=Some(new);
        self.active_row=None;self.bulk_column=0;self.filter.clear();
        self.undo_bulk=None;self.dirty=false;
        let template=self.template.as_ref().unwrap();
        let note=if template.example{" Демонстрационные значения образца пропущены."}else{""};
        self.info(format!("Шаблон: {}. Столбцов: {}, документов: {}.{}",
            template.path.file_name().unwrap_or_default().to_string_lossy(),
            template.headers.len(),self.rows.len(),note));
    }
    fn choose_folder(&mut self){
        if let Some(dir)=rfd::FileDialog::new().set_directory(&self.folder).pick_folder(){
            if self.dirty{self.pending_folder=Some(dir)}
            else{self.open_folder(dir)}
        }
    }
    fn choose_template(&mut self){
        if let Some(path)=rfd::FileDialog::new().set_directory(&self.folder)
            .add_filter("CSV и TSV",&["csv","tsv","txt"]).pick_file(){
            if path.parent()!=Some(self.folder.as_path()){
                self.fail("CSV-шаблон должен находиться в папке с документами.");
                return;
            }
            let project=if let Some(previous)=&self.template {
                Some(Project {
                    version:2,folder:self.folder.clone(),template_path:previous.path.clone(),
                    headers:previous.headers.clone(),rows:self.rows.clone(),
                    title_field:previous.title_column.and_then(|i|previous.headers.get(i).cloned()),
                    asset_field:previous.asset_column.and_then(|i|previous.headers.get(i).cloned()),
                    mapping_configured:true,
                })
            } else {None};
            self.open_template_path(path,project);
        }
    }
    fn refresh(&mut self){
        let Some(template)=self.template.clone() else{
            self.open_folder(self.folder.clone());return;
        };
        match read_template(&template.path){
            Ok(new)=>{
                let old=Project {
                    version:2,folder:self.folder.clone(),template_path:template.path,
                    title_field:template.title_column.and_then(|i|template.headers.get(i).cloned()),
                    asset_field:template.asset_column.and_then(|i|template.headers.get(i).cloned()),
                    mapping_configured:true,
                    headers:template.headers,rows:self.rows.clone(),
                };
                apply_project_mapping(&old,&mut new);
                let mut rows=restore_project_rows(old,&new);
                relink_document_sources(&mut rows,&self.folder);
                let scanned=match scan(&self.folder,&std::env::current_exe().unwrap_or_default(),&new){
                    Ok(rows)=>rows,Err(e)=>{self.fail(format!("Ошибка сканирования: {e}"));return;}
                };
                // Preserve existing work; append documents that were newly added to the folder.
                let known:HashSet<PathBuf>=rows.iter().filter(|r|!r.source.as_os_str().is_empty())
                    .map(|r|r.source.clone()).collect();
                rows.extend(scanned.into_iter().filter(|r|!known.contains(&r.source)));
                self.rows=rows;self.template=Some(new);self.active_row=None;
                self.dirty=true;self.undo_bulk=None;
                self.info("Шаблон перечитан, новые файлы добавлены. Изменения полей сохранены.");
            }
            Err(e)=>self.fail(e),
        }
    }
    fn persist(&mut self){
        let Some(template)=&self.template else{self.fail("Сначала загрузите CSV-шаблон.");return;};
        match save_project(&self.folder,&self.rows,template){
            Ok(_)=>{self.dirty=false;self.info("Проект сохранён.");}
            Err(e)=>self.fail(e),
        }
    }
    fn generate(&mut self){
        let Some(template)=&self.template else{self.fail("Сначала загрузите CSV-шаблон.");return;};
        match export(&self.folder,&self.rows,template){
            Ok(path)=>{
                self.last_export=Some(path.clone());
                match save_project(&self.folder,&self.rows,template){
                    Ok(_)=>{self.dirty=false;self.info(format!("Создан: {}",path.display()));}
                    Err(e)=>{self.dirty=true;self.fail(format!("Архив создан, но проект не сохранён: {e}"));}
                }
            }
            Err(e)=>self.fail(e),
        }
    }
    fn choose_project(&mut self){
        if let Some(path)=rfd::FileDialog::new().set_directory(&self.folder)
            .add_filter("Проект JSON",&["json","bak"]).pick_file(){
            match load_project(&path){
                Ok(project)=>{
                    let folder=path.parent().unwrap_or(Path::new(".")).to_path_buf();
                    self.folder=folder.clone();
                    self.template=None;self.rows.clear();
                    let preferred=project.template_path.file_name()
                        .map(|n|folder.join(n)).filter(|p|p.is_file());
                    let next=preferred.or_else(||template_filename(&folder));
                    if let Some(template)=next{
                        self.open_template_path(template,Some(project));
                        self.info("Проект загружен.");
                    }else{
                        self.fail("Данные проекта прочитаны, но CSV-шаблон рядом не найден. Скопируйте его в эту папку.");
                    }
                }
                Err(e)=>self.fail(format!("Ошибка открытия проекта: {e}"))
            }
        }
    }
    fn add_blank_row(&mut self){
        let Some(t)=&self.template else{return;};
        self.rows.push(Document{source:PathBuf::new(),fields:vec![String::new();t.headers.len()],included:true});
        self.active_row=Some(self.rows.len()-1);self.dirty=true;self.show_editor=true;
    }
    fn delete_selected(&mut self){
        if let Some(idx)=self.active_row{
            if idx<self.rows.len(){self.rows.remove(idx);self.dirty=true;}
            self.active_row=None;
        }
    }
    fn add_files(&mut self){
        let Some(t)=self.template.clone() else{return;};
        let Some(asset)=t.asset_column else{
            self.show_mapping=true;
            self.fail("Сначала выберите колонку для имени вложения в «Колонки файлов».");
            return;
        };
        let files=rfd::FileDialog::new().set_directory(&self.folder).pick_files();
        let Some(files)=files else{return;};
        let mut used:HashSet<String>=self.rows.iter().filter(|r|!r.source.as_os_str().is_empty())
            .filter_map(|r|r.fields.get(asset)).map(|v|v.to_lowercase()).collect();
        let mut count=0;
        for source in files{
            if !source.is_file() || self.rows.iter().any(|r|r.source==source){continue;}
            let file=source.file_name().unwrap_or_default().to_string_lossy().into_owned();
            let mut fields=vec![String::new();t.headers.len()];
            if let Some(title)=t.title_column{
                fields[title]=name_parts(&file).0.to_owned();
            }
            let base=clean_import_filename(&file);
            let mut counter=1;
            loop{
                let filename=number_suffix(&base,counter);
                if used.insert(filename.to_lowercase()){
                    fields[asset]=filename;
                    break;
                }
                counter+=1;
            }
            self.rows.push(Document{source,fields,included:true});count+=1;
        }
        if count>0{self.dirty=true;}
        self.info(format!("Добавлено файлов: {count}"));
    }
    fn mapping_bar(&mut self,ui:&mut egui::Ui){
        if !self.show_mapping{return;}
        let Some(t)=self.template.as_mut() else{return;};
        let headers=t.headers.clone();
        let before=(t.title_column,t.asset_column);
        ui.horizontal(|ui|{
            ui.label(RichText::new("Название:").color(dim(self.dark)));
            egui::ComboBox::from_id_salt("title_role")
                .selected_text(t.title_column.and_then(|i|headers.get(i))
                    .map(String::as_str).unwrap_or("Не назначено"))
                .width(155.0).show_ui(ui,|ui|{
                    ui.selectable_value(&mut t.title_column,None,"Не назначено");
                    for (i,label) in headers.iter().enumerate(){
                        ui.selectable_value(&mut t.title_column,Some(i),label);
                    }
                });
            ui.label(RichText::new("Имя вложения:").color(dim(self.dark)));
            egui::ComboBox::from_id_salt("asset_role")
                .selected_text(t.asset_column.and_then(|i|headers.get(i))
                    .map(String::as_str).unwrap_or("Не назначено"))
                .width(155.0).show_ui(ui,|ui|{
                    ui.selectable_value(&mut t.asset_column,None,"Не назначено");
                    for (i,label) in headers.iter().enumerate(){
                        ui.selectable_value(&mut t.asset_column,Some(i),label);
                    }
                });
            ui.label(RichText::new("Назначение необязательно для CSV без вложений.").size(11.0)
                .color(dim(self.dark)));
        });
        if before!=(t.title_column,t.asset_column){self.dirty=true;}
    }
    fn bulk_apply(&mut self,clear:bool){
        let Some(template)=&self.template else{return;};
        if self.bulk_column>=template.headers.len(){return;}
        self.undo_bulk=Some(self.rows.iter().map(|row|row.fields.clone()).collect());
        let value=if clear{""}else{&self.bulk_value};
        let changed=bulk_edit(&mut self.rows,self.bulk_column,value,self.bulk_checked);
        if changed>0{self.dirty=true;}
        self.info(format!("Изменено {} строк в колонке {}",changed,template.headers[self.bulk_column]));
    }
    fn undo(&mut self){
        if let Some(old)=self.undo_bulk.take(){
            for (row,values) in self.rows.iter_mut().zip(old){row.fields=values;}
            self.dirty=true;self.info("Массовая правка отменена.");
        }
    }
    fn filtered(&self)->Vec<usize>{
        let needle=self.filter.trim().to_lowercase();
        self.rows.iter().enumerate()
            .filter(|(_,row)|!self.only_included || row.included)
            .filter(|(_,row)|needle.is_empty()
                || row.fields.iter().any(|field|field.to_lowercase().contains(&needle))
                || row.source.to_string_lossy().to_lowercase().contains(&needle))
            .map(|(i,_)|i).collect()
    }
    fn toolbar(&mut self,ui:&mut egui::Ui){
        ui.horizontal(|ui|{
            ui.label(RichText::new("Импорт документов").size(18.0).strong().color(ink(self.dark)));
            ui.label(RichText::new("v6.3").size(11.0).color(dim(self.dark)));
            ui.separator();
            if ui.add(button("Папка",self.dark)).clicked(){self.choose_folder();}
            if ui.add(button("Шаблон CSV",self.dark)).clicked(){self.choose_template();}
            if ui.add(button("Открыть проект",self.dark)).clicked(){self.choose_project();}
            if ui.add(button("Обновить",self.dark)).clicked(){
                if self.dirty{self.confirm_rescan=true}else{self.refresh();}
            }
            ui.with_layout(egui::Layout::right_to_left(Align::Center),|ui|{
                if ui.add_enabled(self.template.is_some(),action("Создать ZIP")).clicked(){self.generate();}
                if let Some(path)=self.last_export.clone(){
                    if ui.add(button("Показать результат",self.dark)).clicked(){
                        #[cfg(windows)]
                        {
                            if let Some(folder)=path.parent(){
                                if let Err(e)=std::process::Command::new("explorer.exe").arg(folder).spawn(){
                                    self.fail(format!("Не удалось открыть папку: {e}"));
                                }
                            }
                        }
                        #[cfg(not(windows))]
                        {self.info(format!("Результат: {}",path.display()));}
                    }
                }
                if ui.add_enabled(self.template.is_some(),button("Сохранить проект",self.dark)).clicked(){self.persist();}
                if ui.add(button(if self.dark{"Светлая"}else{"Тёмная"},self.dark)).clicked(){
                    self.dark=!self.dark;apply_theme(ui.ctx(),self.dark);
                }
            });
        });
        ui.horizontal(|ui|{
            ui.label(RichText::new(self.folder.display().to_string()).size(11.0).color(dim(self.dark)));
            ui.separator();
            if let Some(t)=&self.template{
                ui.label(RichText::new(format!("Шаблон: {} [{}]",
                    t.path.file_name().unwrap_or_default().to_string_lossy(),t.headers.len()))
                    .size(11.0).color(dim(self.dark)));
            } else {
                ui.colored_label(BAD,"Нет шаблона");
            }
            ui.separator();
            ui.label(RichText::new(format!("Строк: {} / {}",self.rows.iter()
                .filter(|r|r.included).count(),self.rows.len())).size(11.0).color(dim(self.dark)));
            if self.dirty{ui.colored_label(BAD,"Не сохранено");}
        });
    }
    fn bulk_bar(&mut self,ui:&mut egui::Ui){
        let Some(t)=&self.template else{return;};
        let headers=t.headers.clone();
        ui.horizontal_wrapped(|ui|{
            if ui.add(button(if self.show_bulk{"Скрыть массовую правку"}else{"Массовая правка"},self.dark)).clicked(){
                self.show_bulk=!self.show_bulk;
            }
            if self.show_bulk{
                egui::ComboBox::from_id_salt("bulk_column_v6")
                    .selected_text(headers.get(self.bulk_column).map(|s|localized(s)).unwrap_or("Колонка"))
                    .width(155.0).show_ui(ui,|ui|{
                        for (i,name) in headers.iter().enumerate(){
                            ui.selectable_value(&mut self.bulk_column,i,format!("{} · {}",localized(name),name));
                        }
                    });
                ui.add(TextEdit::singleline(&mut self.bulk_value).hint_text("Общее значение").desired_width(240.0));
                ui.checkbox(&mut self.bulk_checked,"Отмеченные");
                if ui.add(action("Применить")).clicked(){self.bulk_apply(false);}
                if ui.add(button("Очистить",self.dark)).clicked(){self.bulk_apply(true);}
                if ui.add_enabled(self.undo_bulk.is_some(),button("Отменить",self.dark)).clicked(){self.undo();}
            }
        });
    }
    fn filterbar(&mut self,ui:&mut egui::Ui){
        ui.horizontal(|ui|{
            ui.add(TextEdit::singleline(&mut self.filter).hint_text("Поиск по таблице")
                .desired_width(240.0));
            ui.checkbox(&mut self.only_included,"Отмеченные");
            if ui.add(button("Все",self.dark)).clicked(){
                for row in &mut self.rows{row.included=true;}self.dirty=true;
            }
            if ui.add(button("Снять",self.dark)).clicked(){
                for row in &mut self.rows{row.included=false;}self.dirty=true;
            }
            if ui.add_enabled(self.template.is_some(),button("+ Строка",self.dark)).clicked(){self.add_blank_row();}
            if ui.add_enabled(self.template.is_some(),button("+ Файлы",self.dark)).clicked(){self.add_files();}
            if ui.add_enabled(self.active_row.is_some(),button("Удалить",self.dark)).clicked(){self.delete_selected();}
            ui.with_layout(egui::Layout::right_to_left(Align::Center),|ui|{
                if ui.add(button(if self.show_mapping{"Скрыть назначение"}else{"Колонки файлов"},self.dark)).clicked(){
                    self.show_mapping=!self.show_mapping;
                }
                if ui.add(button(if self.show_editor{"Скрыть поля"}else{"Поля справа"},self.dark)).clicked(){
                    self.show_editor=!self.show_editor;
                }
                if ui.add(button(if self.full_columns{"Кратко"}else{"Все столбцы"},self.dark)).clicked(){
                    self.full_columns=!self.full_columns;
                }
            });
        });
    }
    fn table(&mut self,ui:&mut egui::Ui){
        let Some(template)=&self.template else{
            ui.add_space(18.0);
            ui.heading("Не найден CSV-шаблон");
            ui.label("Выберите любой CSV-шаблон. Названия колонок могут быть произвольными.");
            return;
        };
        let visible=self.filtered();
        let cols:Vec<usize>=if self.full_columns{(0..template.headers.len()).collect()}
            else{prime_columns(template)};
        let headers=template.headers.clone();
        let dark=self.dark;
        let min=80.0+cols.iter().map(|&c|initial_width(&headers[c])).sum::<f32>();
        egui::ScrollArea::horizontal().id_salt("v6_hscroll").auto_shrink([false,false])
            .show(ui,|ui|{
                ui.set_min_width(min.max(ui.available_width()));
                let mut table=TableBuilder::new(ui)
                    .id_salt(if self.full_columns{"v6_full"}else{"v6_basic"})
                    .striped(true).resizable(true).auto_shrink([false,false])
                    .column(Column::exact(28.0)).column(Column::exact(36.0));
                for &c in &cols{
                    table=table.column(Column::initial(initial_width(&headers[c]))
                        .at_least(65.0).at_most(1200.0).resizable(true).clip(true));
                }
                table.header(25.0,|mut header|{
                    header.col(|ui|{ui.label("В");});
                    header.col(|ui|{ui.label("#");});
                    for &c in &cols{
                        header.col(|ui|{
                            ui.label(RichText::new(localized(&headers[c])).strong().color(ink(dark)))
                                .on_hover_text(format!("{} — перетащите границу для изменения ширины",headers[c]));
                        });
                    }
                }).body(|body|{
                    body.rows(27.0,visible.len(),|mut row|{
                        let idx=visible[row.index()];
                        let doc=&mut self.rows[idx];
                        row.col(|ui|{
                            if ui.checkbox(&mut doc.included,"").changed(){self.dirty=true;}
                        });
                        row.col(|ui|{
                            if ui.selectable_label(self.active_row==Some(idx),(idx+1).to_string()).clicked(){
                                self.active_row=Some(idx);self.show_editor=true;
                            }
                        });
                        for &c in &cols{
                            row.col(|ui|{
                                if let Some(value)=doc.fields.get_mut(c){
                                    if ui.add(TextEdit::singleline(value).desired_width(f32::INFINITY)
                                        .clip_text(true).margin(egui::vec2(3.0,2.0))).changed(){
                                        self.dirty=true;
                                    }
                                }
                            });
                        }
                    });
                });
            });
    }
    fn inspector(&mut self,ui:&mut egui::Ui){
        ui.horizontal(|ui|{
            ui.label(RichText::new("Поля документа").strong().color(ink(self.dark)));
            ui.with_layout(egui::Layout::right_to_left(Align::Center),|ui|{
                if ui.small_button("Закрыть").clicked(){self.show_editor=false;}
            });
        });
        let Some(template)=&self.template else{return;};
        let Some(index)=self.active_row else{
            ui.label(RichText::new("Выберите номер строки.").size(11.0).color(dim(self.dark)));
            return;
        };
        if index>=self.rows.len(){self.active_row=None;return;}
        let fields=template.headers.clone();
        let file_idx=template.asset_column;
        let filename=self.rows[index].source.file_name()
            .map(|n|n.to_string_lossy().into_owned())
            .unwrap_or_else(||"Без прикреплённого файла".to_owned());
        ui.label(RichText::new(filename).size(11.0).color(dim(self.dark)));
        ui.horizontal(|ui|{
            if ui.small_button("Прикрепить файл").clicked(){
                if let Some(col)=template.asset_column{
                    if let Some(path)=rfd::FileDialog::new().set_directory(&self.folder).pick_file(){
                        let name=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                        self.rows[index].source=path;
                        self.rows[index].fields[col]=clean_import_filename(&name);
                        self.rows[index].included=true;
                        self.dirty=true;
                    }
                } else {
                    self.show_mapping=true;
                    self.fail("Назначьте колонку имени вложения в «Колонки файлов».");
                }
            }
            if !self.rows[index].source.as_os_str().is_empty()
                && ui.small_button("Убрать вложение").clicked(){
                    self.rows[index].source=PathBuf::new();
                    self.dirty=true;
                }
        });
        ui.separator();
        egui::ScrollArea::vertical().id_salt("v6_fields").show(ui,|ui|{
            for (col,heading) in fields.iter().enumerate(){
                ui.horizontal(|ui|{
                    ui.label(RichText::new(localized(heading)).strong().color(ink(self.dark)));
                    ui.label(RichText::new(heading).size(10.0).color(dim(self.dark)));
                });
                if let Some(value)=self.rows[index].fields.get_mut(col){
                    let long=matches!(heading.to_ascii_lowercase().as_str(),"description"|"hypertext"|"graphic"|"eds");
                    if long{
                        if ui.add(TextEdit::multiline(value).desired_rows(2)
                            .desired_width(f32::INFINITY)).changed(){self.dirty=true;}
                    }else if ui.add(TextEdit::singleline(value)
                        .desired_width(f32::INFINITY)).changed(){self.dirty=true;}
                }
                if file_idx==Some(col){
                    if ui.small_button("Сделать имя из названия").clicked(){
                        if let Some(name_idx)=template.title_column{
                            let stem=self.rows[index].fields[name_idx].clone();
                            let ext=self.rows[index].source.extension()
                                .and_then(|x|x.to_str()).unwrap_or("");
                            let proposed=if ext.is_empty(){stem}else{format!("{stem}.{ext}")};
                            self.rows[index].fields[col]=clean_import_filename(&proposed);
                            self.dirty=true;
                        }
                    }
                }
                ui.add_space(2.0);
            }
        });
    }
    fn footer(&mut self,ui:&mut egui::Ui){
        ui.horizontal(|ui|{
            let color=if self.error{BAD}else{if self.dark{GOOD}else{Color32::from_rgb(19,119,91)}};
            ui.label(RichText::new(&self.status).size(11.0).color(color));
            ui.with_layout(egui::Layout::right_to_left(Align::Center),|ui|{
                ui.label(RichText::new("CSV без BOM · Ctrl+S сохранить · Ctrl+E экспорт")
                    .size(11.0).color(dim(self.dark)));
            });
        });
    }
    fn template_selection_window(&mut self,ctx:&egui::Context){
        if self.template_options.is_empty(){return;}
        let candidates=self.template_options.clone();
        egui::Window::new("Выберите CSV-шаблон").collapsible(false)
            .resizable(false).anchor(egui::Align2::CENTER_CENTER,[0.0,0.0])
            .show(ctx,|ui|{
                ui.label("В папке несколько подходящих файлов. Укажите тот, который предоставил сайт.");
                ui.add_space(6.0);
                for path in candidates {
                    let title=path.file_name().unwrap_or_default().to_string_lossy().into_owned();
                    let note=read_template(&path).ok()
                        .map(|t|format!(" · {} колонок",t.headers.len())).unwrap_or_default();
                    if ui.button(format!("{title}{note}")).clicked(){
                        let project=self.pending_project.take();
                        self.template_options.clear();
                        self.open_template_path(path,project);
                    }
                }
                ui.add_space(4.0);
                if ui.button("Выбрать другой файл...").clicked(){
                    self.template_options.clear();self.pending_project=None;
                    self.choose_template();
                }
            });
    }
    fn confirmation(&mut self,ctx:&egui::Context){
        if self.confirm_rescan{
            egui::Window::new("Перечитать шаблон и обновить список?").collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER,[0.0,0.0]).show(ctx,|ui|{
                    ui.label("Рекомендуется сохранить проект перед обновлением.");
                    ui.horizontal(|ui|{
                        if ui.button("Сохранить и обновить").clicked(){
                            self.persist();
                            if !self.error{self.refresh();self.confirm_rescan=false;}
                        }
                        if ui.button("Обновить без сохранения").clicked(){
                            self.refresh();self.confirm_rescan=false;
                        }
                        if ui.button("Отмена").clicked(){self.confirm_rescan=false;}
                    });
                });
        }
        if let Some(folder)=self.pending_folder.clone(){
            egui::Window::new("Сохранить проект?").collapsible(false)
                .anchor(egui::Align2::CENTER_CENTER,[0.0,0.0]).show(ctx,|ui|{
                    ui.label("Сохранить изменения перед сменой папки?");
                    ui.horizontal(|ui|{
                        if ui.button("Сохранить и открыть").clicked(){
                            self.persist();
                            if !self.error{self.open_folder(folder.clone());self.pending_folder=None;}
                        }
                        if ui.button("Не сохранять").clicked(){self.open_folder(folder.clone());self.pending_folder=None;}
                        if ui.button("Отмена").clicked(){self.pending_folder=None;}
                    });
                });
        }
    }
}
impl eframe::App for ImporterApp {
    fn update(&mut self,ctx:&egui::Context,_frame:&mut eframe::Frame){
        if ctx.input(|i|i.modifiers.ctrl && i.key_pressed(egui::Key::S)){self.persist();}
        if ctx.input(|i|i.modifiers.ctrl && i.key_pressed(egui::Key::E)){self.generate();}
        if ctx.input(|i|i.modifiers.ctrl && i.key_pressed(egui::Key::O)){self.choose_folder();}
        if ctx.input(|i|i.key_pressed(egui::Key::F5)){if self.dirty{self.confirm_rescan=true;}else{self.refresh();}}
        egui::TopBottomPanel::top("v6_toolbar")
            .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::symmetric(12,7)))
            .show(ctx,|ui|self.toolbar(ui));
        egui::TopBottomPanel::top("v6_bulk")
            .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::symmetric(12,3)))
            .show(ctx,|ui|self.bulk_bar(ui));
        if self.show_mapping{
            egui::TopBottomPanel::top("v6_mapping")
                .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::symmetric(12,3)))
                .show(ctx,|ui|self.mapping_bar(ui));
        }
        egui::TopBottomPanel::bottom("v6_status")
            .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::symmetric(12,5)))
            .show(ctx,|ui|self.footer(ui));
        if self.show_editor{
            egui::SidePanel::right("v6_inspector").resizable(true)
                .default_width(295.0).min_width(230.0)
                .frame(Frame::new().fill(surface(self.dark)).inner_margin(Margin::same(10)))
                .show(ctx,|ui|self.inspector(ui));
        }
        egui::CentralPanel::default()
            .frame(Frame::new().fill(canvas(self.dark)).inner_margin(Margin::symmetric(7,7)))
            .show(ctx,|ui|{
                self.filterbar(ui);ui.separator();self.table(ui);
            });
        self.template_selection_window(ctx);
        self.confirmation(ctx);
    }
}
pub(super) fn launch()->eframe::Result{
    let options=eframe::NativeOptions{
        viewport:egui::ViewportBuilder::default()
            .with_inner_size([1370.0,790.0])
            .with_min_inner_size([860.0,520.0]),
        ..Default::default()
    };
    eframe::run_native("Импорт документов",options,
        Box::new(|cc|Ok(Box::new(ImporterApp::new(cc)))))
}

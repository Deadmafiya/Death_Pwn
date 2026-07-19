//! File listing types and icon mapping for the bottom bar.

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClickAction {
    ToggleTarget { ip: String },
    NavigateDir { path: String },
    OpenFile { path: String },
    CopyToClipboard { text: String },
}

#[derive(Debug, Clone)]
pub struct ClickItem {
    pub action: ClickAction,
    pub row_y: u16,
    pub col_range: Option<(u16, u16)>,
}

#[derive(Debug, Clone)]
pub struct FileEntry {
    pub name: String,
    pub is_dir: bool,
    pub icon: char,
}

pub fn icon_for_entry(name: &str, is_dir: bool) -> char {
    if is_dir {
        return '\u{f07b}';
    }
    if name.starts_with('.') && name.len() == 1 {
        return '\u{f07b}';
    }
    let lower = name.to_lowercase();
    if lower.ends_with(".rs")              { '\u{e7a8}' }
    else if lower.ends_with(".py")         { '\u{e73c}' }
    else if lower.ends_with(".sh")         { '\u{f489}' }
    else if lower.ends_with(".toml")       { '\u{e6c5}' }
    else if lower.ends_with(".json")       { '\u{e60b}' }
    else if lower.ends_with(".md")         { '\u{f48a}' }
    else if lower.ends_with(".lock")       { '\u{f023}' }
    else if lower.ends_with(".txt")        { '\u{f15c}' }
    else if lower.ends_with(".yaml") || lower.ends_with(".yml") { '\u{e6c5}' }
    else if lower.ends_with(".c")          { '\u{e61e}' }
    else if lower.ends_with(".h")          { '\u{f0fd}' }
    else if lower.ends_with(".cpp") || lower.ends_with(".cc") || lower.ends_with(".cxx") { '\u{e61d}' }
    else if lower.ends_with(".go")         { '\u{e626}' }
    else if lower.ends_with(".js")         { '\u{e74e}' }
    else if lower.ends_with(".ts")         { '\u{e628}' }
    else if lower.ends_with(".jsx")        { '\u{e7ba}' }
    else if lower.ends_with(".tsx")        { '\u{e7ba}' }
    else if lower.ends_with(".html")       { '\u{f13b}' }
    else if lower.ends_with(".css")        { '\u{e749}' }
    else if lower.ends_with(".scss")       { '\u{e749}' }
    else if lower.ends_with(".php")        { '\u{e73d}' }
    else if lower.ends_with(".rb")         { '\u{e21e}' }
    else if lower.ends_with(".pl")         { '\u{e769}' }
    else if lower.ends_with(".nse")        { '\u{f233}' }
    else if lower.ends_with(".conf")       { '\u{e615}' }
    else if lower.ends_with(".cfg")        { '\u{e615}' }
    else if lower.ends_with(".ini")        { '\u{e615}' }
    else if lower.ends_with(".xml")        { '\u{f121}' }
    else if lower.ends_with(".svg")        { '\u{e7b4}' }
    else if lower.ends_with(".png") || lower.ends_with(".jpg") || lower.ends_with(".jpeg")
         || lower.ends_with(".gif") || lower.ends_with(".bmp") { '\u{f1c5}' }
    else if lower.ends_with(".mp3") || lower.ends_with(".wav")
         || lower.ends_with(".ogg") || lower.ends_with(".flac") { '\u{f1c7}' }
    else if lower.ends_with(".mp4") || lower.ends_with(".mkv")
         || lower.ends_with(".avi") || lower.ends_with(".webm") { '\u{f1c8}' }
    else if lower.ends_with(".zip") || lower.ends_with(".tar") || lower.ends_with(".gz")
         || lower.ends_with(".xz") || lower.ends_with(".bz2") || lower.ends_with(".7z")
         || lower.ends_with(".rar") { '\u{f1c6}' }
    else if lower.ends_with(".exe") || lower.ends_with(".bin")
         || lower.ends_with(".elf") || lower.ends_with(".out") { '\u{f013}' }
    else if lower.ends_with(".pdf")        { '\u{f1c1}' }
    else if lower.ends_with(".sql")        { '\u{f1c0}' }
    else if lower.ends_with(".log")        { '\u{f18c}' }
    else if lower.ends_with(".key") || lower.ends_with(".pem")
         || lower.ends_with(".pub")        { '\u{f084}' }
    else if lower.ends_with(".env")        { '\u{e615}' }
    else if lower.ends_with(".gitignore")  { '\u{f1d3}' }
    else if lower.ends_with(".dockerfile") { '\u{f308}' }
    else                                   { '\u{f016}' }
}

pub fn refresh_file_list(dir: &str) -> Vec<FileEntry> {
    let mut entries = Vec::new();
    let path = std::path::Path::new(dir);
    if let Ok(read_dir) = std::fs::read_dir(path) {
        let mut items: Vec<_> = read_dir
            .filter_map(|e| e.ok())
            .map(|e| {
                let name = e.file_name().to_string_lossy().to_string();
                let is_dir = e.file_type().map(|t| t.is_dir()).unwrap_or(false);
                (name, is_dir)
            })
            .collect();
        items.sort_by(|a, b| {
            b.1.cmp(&a.1)
                .then_with(|| a.0.to_lowercase().cmp(&b.0.to_lowercase()))
        });

        for (name, is_dir) in items {
            let icon = icon_for_entry(&name, is_dir);
            entries.push(FileEntry {
                name,
                is_dir,
                icon,
            });
        }
    }
    entries
}

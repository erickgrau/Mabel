use chrono::Local;
use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;

pub fn append(app_dir: &PathBuf, message: &str) {
    if std::fs::create_dir_all(app_dir).is_err() {
        return;
    }
    let path = app_dir.join("debug.log");
    let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
    if let Ok(mut f) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(f, "[{}] {}", ts, message);
    }
}

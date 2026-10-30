use std::{
    fs,
    io::{self, Write},
    path::PathBuf,
};

fn path() -> Option<PathBuf> {
    std::env::var_os("HOME").map(|home| {
        PathBuf::from(home).join("Library/Application Support/Light Usage Bar/preferences.json")
    })
}

pub(crate) fn load() -> [bool; 4] {
    path()
        .and_then(|path| {
            let file = fs::File::open(path).ok()?;
            let bytes = std::io::Read::take(file, 4097);
            serde_json::from_reader::<_, [bool; 4]>(bytes).ok()
        })
        .unwrap_or([true; 4])
}

pub(crate) fn save(enabled: [bool; 4]) -> io::Result<()> {
    let path = path().ok_or_else(|| io::Error::new(io::ErrorKind::NotFound, "home unavailable"))?;
    let directory = path.parent().expect("preferences parent");
    fs::create_dir_all(directory)?;
    let temporary = directory.join(format!("preferences.{}.tmp", std::process::id()));
    let mut options = fs::OpenOptions::new();
    options.write(true).create(true).truncate(true);
    #[cfg(unix)]
    {
        use std::os::unix::fs::OpenOptionsExt;
        options.mode(0o600);
    }
    let mut file = options.open(&temporary)?;
    serde_json::to_writer(&mut file, &enabled)?;
    file.flush()?;
    file.sync_all()?;
    fs::rename(temporary, path)
}

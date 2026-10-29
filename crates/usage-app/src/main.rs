#[cfg(target_os = "macos")]
mod appkit;
#[cfg(target_os = "macos")]
fn main() {
    if std::env::args().skip(1).collect::<Vec<_>>().as_slice() == ["--credential-helper"] {
        usage_service::credential_helper_main();
    }
    appkit::run();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Light Usage Bar requires macOS.");
    std::process::exit(1);
}

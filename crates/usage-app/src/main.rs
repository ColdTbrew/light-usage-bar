#[cfg(target_os = "macos")]
mod appkit;
#[cfg(target_os = "macos")]
fn main() {
    appkit::run();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Light Usage Bar requires macOS.");
    std::process::exit(1);
}

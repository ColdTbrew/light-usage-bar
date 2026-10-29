#[cfg(any(target_os = "macos", test))]
mod lifecycle;
#[cfg(target_os = "macos")]
mod panel;
#[cfg(any(target_os = "macos", test))]
mod placement;
#[cfg(target_os = "macos")]
fn main() {
    panel::run();
}
#[cfg(not(target_os = "macos"))]
fn main() {
    eprintln!("Light Usage Panel requires macOS.");
    std::process::exit(1);
}

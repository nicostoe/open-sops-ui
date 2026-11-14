//! Test-only stand-in for the desktop app's editor mode. The real app calls
//! `run_as_editor_if_requested` at the top of its own `main`.
fn main() {
    std::process::exit(sops_env_core::run_as_editor_if_requested().unwrap_or(2));
}

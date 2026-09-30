#[cfg(any(target_arch = "wasm32", test))]
mod interaction;

#[cfg(target_arch = "wasm32")]
mod app;

#[cfg(target_arch = "wasm32")]
fn main() {
    console_error_panic_hook::set_once();
    leptos::mount::mount_to_body(app::App);
}

#[cfg(not(target_arch = "wasm32"))]
fn main() {
    eprintln!("Build the Software Factory web UI for wasm32-unknown-unknown with Trunk.");
}

mod app;
mod components;
mod services;

fn main() {
    leptos::mount::mount_to_body(app::App);
}

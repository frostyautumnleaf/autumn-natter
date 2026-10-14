// Tauri reads tauri.conf.json here, puts the page of the front end into the
// program, and does the steps that a bundle needs. Nothing else is built.
fn main() {
    tauri_build::build();
}

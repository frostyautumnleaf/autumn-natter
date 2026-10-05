// Builds the Slint UI files before the Rust code compiles.
fn main() {
    slint_build::compile("ui/app.slint").expect("the Slint UI must compile");
    // Rerun the build step when a UI file changes.
    println!("cargo:rerun-if-changed=ui/app.slint");
    println!("cargo:rerun-if-changed=ui/theme.slint");
    println!("cargo:rerun-if-changed=ui/widgets.slint");
    println!("cargo:rerun-if-changed=ui/types.slint");
    println!("cargo:rerun-if-changed=ui/model_config.slint");
    println!("cargo:rerun-if-changed=ui/settings.slint");
    // The browser page of the remote mode and the icon of the page are put into
    // the program when it is built.
    println!("cargo:rerun-if-changed=web/page.html");
    println!("cargo:rerun-if-changed=assets/icons/autumn-natter.svg");
}

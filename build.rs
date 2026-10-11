// Autumn Natter has no code to make. This step only tells Cargo when to build
// again. Two files are put into the program while it is built, so a change to
// either of them must start a new build.
fn main() {
    // The browser page of the remote mode.
    println!("cargo:rerun-if-changed=web/page.html");
    // The mark of the program, used by the window and by the page.
    println!("cargo:rerun-if-changed=assets/icons/autumn-natter.svg");
}

//! Embeds the app icon and version into folio.exe on Windows.

fn main() {
    println!("cargo:rerun-if-changed=resources/folio.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("resources/folio.ico");
        res.set("ProductName", "folio");
        res.set("FileDescription", "folio — documents, sheets and slides");
        if let Err(e) = res.compile() {
            println!("cargo:warning=couldn't embed the Windows icon: {e}");
        }
    }
}

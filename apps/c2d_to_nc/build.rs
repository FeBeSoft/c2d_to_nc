//! Intègre l'icône et les informations de version dans l'exécutable Windows.
//! Sans outil de ressources (rc.exe absent), le build continue avec un simple avertissement.

fn main() {
    let icon = "../../assets/c2d_to_nc.ico";
    println!("cargo:rerun-if-changed={icon}");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let mut res = winresource::WindowsResource::new();
    res.set_icon(icon)
        .set("ProductName", "c2d_to_nc")
        .set("FileDescription", "c2d_to_nc — projet .c2d vers G-code Grbl")
        .set("LegalCopyright", "GPL-3.0-or-later — c2d_to_nc contributors");
    if let Err(e) = res.compile() {
        println!("cargo:warning=icône non intégrée : {e}");
    }
}

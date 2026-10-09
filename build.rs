use std::env;
use std::path::PathBuf;

fn main() {
    // The window text's translations come from the same catalogs as Rust's (see src/i18n.rs).
    // Messages carry no context unless they name one, so both sides look them up alike.
    let config = slint_build::CompilerConfiguration::new()
        .with_bundled_translations("lang")
        .with_default_translation_context(slint_build::DefaultTranslationContext::None);
    slint_build::compile_with_config("ui/app.slint", config).expect("failed to compile the Slint UI");
    windows_resources();
}

/// Gives wisp.exe its icon, version information and manifest, so Explorer, the
/// taskbar, Task Manager and the startup-apps list show "Wisp" with its icon.
fn windows_resources() {
    if env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    let root = PathBuf::from(env::var("CARGO_MANIFEST_DIR").expect("set by cargo"));
    let out = PathBuf::from(env::var("OUT_DIR").expect("set by cargo"));
    let version = env::var("CARGO_PKG_VERSION").expect("set by cargo");
    let mut numbers: Vec<u16> = version.split(['.', '-', '+']).map_while(|p| p.parse().ok()).collect();
    numbers.resize(4, 0);
    let numeric = numbers.iter().map(u16::to_string).collect::<Vec<_>>().join(",");
    let file = |relative: &str| root.join(relative).display().to_string().replace('\\', "/");

    // ID 1 is the icon Explorer shows; the app also loads it for its windows.
    let rc = format!(
        r#"1 ICON "{icon}"
1 24 "{manifest}"
1 VERSIONINFO
FILEVERSION {numeric}
PRODUCTVERSION {numeric}
FILEFLAGSMASK 0x3f
FILEFLAGS 0x0
FILEOS 0x40004
FILETYPE 0x1
FILESUBTYPE 0x0
BEGIN
  BLOCK "StringFileInfo"
  BEGIN
    BLOCK "040904b0"
    BEGIN
      VALUE "CompanyName", "Wisp"
      VALUE "FileDescription", "Wisp"
      VALUE "FileVersion", "{version}"
      VALUE "InternalName", "wisp"
      VALUE "LegalCopyright", "MIT License"
      VALUE "OriginalFilename", "wisp.exe"
      VALUE "ProductName", "Wisp"
      VALUE "ProductVersion", "{version}"
    END
  END
  BLOCK "VarFileInfo"
  BEGIN
    VALUE "Translation", 0x409, 1200
  END
END
"#,
        icon = file("assets/wisp.ico"),
        manifest = file("assets/windows/wisp.manifest"),
    );
    let rc_path = out.join("wisp.rc");
    std::fs::write(&rc_path, rc).expect("write the resource script");
    println!("cargo:rerun-if-changed=assets/wisp.ico");
    println!("cargo:rerun-if-changed=assets/windows/wisp.manifest");
    embed_resource::compile(&rc_path, embed_resource::NONE)
        .manifest_optional()
        .expect("compile Windows resources (needs rc.exe from the Windows SDK, or windres for MinGW)");
}

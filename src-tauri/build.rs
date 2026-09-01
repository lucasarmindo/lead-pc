fn main() {
    // O manifesto requireAdmin fica comentado durante o desenvolvimento:
    // `cargo run`/`tauri dev` usam CreateProcess diretamente, que não consegue
    // iniciar um binário com requireAdmin (só funciona via Explorer/atalho,
    // que passa pelo ShellExecute). Reative isso soment antes do build final.
    //
    // let manifest = r#"<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
    // <assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
    //   <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    //     <security>
    //       <requestedPrivileges>
    //         <requestedExecutionLevel level="requireAdmin" uiAccess="false" />
    //       </requestedPrivileges>
    //     </security>
    //   </trustInfo>
    // </assembly>"#;
    //
    // let attrs = tauri_build::Attributes::new()
    //     .windows_attributes(tauri_build::WindowsAttributes::new().app_manifest(manifest));
    // tauri_build::try_build(attrs).expect("failed to run tauri-build");

    tauri_build::build()
}

fn main() {
    // requireAdministrator no manifesto foi testado e descartado: o WebView2
    // não inicializa corretamente quando o processo host roda elevado (o app
    // quebra logo após o UAC). Em vez de elevar o app inteiro, cada ação que
    // precisa de admin (sfc, DISM, DNS, hibernação, HAGS, etc.) já pede UAC
    // individualmente só para aquele comando — ver run_elevated_capture /
    // run_elevated_and_wait em src/lib.rs. Não reativar isso.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build()
}

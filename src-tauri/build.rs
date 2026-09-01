fn main() {
    // requireAdmin fica desativado durante o desenvolvimento (tauri dev não
    // consegue lançar um binário com esse manifesto). Reativar antes do
    // próximo `tauri build` de release — ver histórico do projeto.
    println!("cargo:rerun-if-changed=icons/icon.ico");
    tauri_build::build()
}

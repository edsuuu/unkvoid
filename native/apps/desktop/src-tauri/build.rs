fn main() {
    // O `generate_context!` embute o frontend no binário em tempo de COMPILAÇÃO. Sem
    // isto o cargo não vê o `dist/` mudar, não recompila, e o app sai com o frontend da
    // última vez que o Rust mudou — no pior caso, com o `dist` vazio de antes do
    // primeiro build. A tela fica preta e a build passa sem um aviso sequer.
    //
    // Foi exatamente isso: várias versões instaladas com a interface antiga embutida.
    println!("cargo:rerun-if-changed=../dist");

    tauri_build::build();
}

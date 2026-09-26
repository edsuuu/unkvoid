fn main() {
    slint_build::compile("ui/app.slint").expect("a interface não compilou");

    // O ícone de dentro do .exe: é o que o Explorer, o atalho e a barra de tarefas mostram. O
    // mesmo desenho do app do Tauri, que este substitui no mesmo lugar.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = tauri_winres::WindowsResource::new();

        resource.set_icon("../desktop/src-tauri/icons/icon.ico");
        resource.compile().expect("o ícone não entrou no .exe");
    }
}

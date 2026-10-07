fn main() {
    slint_build::compile("ui/app.slint").expect("a interface não compilou");

    // O ícone de dentro do .exe: é o que o Explorer, o atalho e a barra de tarefas mostram. O
    // mesmo desenho do app do Tauri, que este substitui no mesmo lugar.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = tauri_winres::WindowsResource::new();

        resource.set_icon("../desktop/src-tauri/icons/icon.ico");

        // É o nome que o Gerenciador de Tarefas e o "Abrir com" mostram. Sem ele vale o nome do
        // pacote do Cargo, e o app aparecia como "unkvoid-windows".
        resource.set("FileDescription", "Unkvoid");
        resource.set("ProductName", "Unkvoid");

        // O app roda elevado numa conta de administrador por causa dos Clips: o anti-cheat de
        // vários jogos (o do Arena Breakout, por exemplo) põe o jogo em nível elevado, e o
        // Windows esconde o teclado e a janela de um jogo elevado de programas comuns — o Alt+Z
        // não chegava e o painel não subia por cima dele.
        //
        // Mas o `.exe` abre sem pedir nada (`asInvoker`), e quem eleva é o próprio app, pela
        // tarefa agendada, que abre elevado sem o aviso do UAC (`clips::shell::elevate`). Com
        // `highestAvailable` aqui, todo clique no ícone fixado na barra de tarefas pedia o UAC —
        // até com o app já aberto na bandeja, só para descobrir que ele já estava rodando.
        //
        // Só no build de release, que é o que o instalador leva: o Cargo põe o recurso do
        // binário também no executável dos testes dele.
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            resource.set_manifest(
                r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="asInvoker" uiAccess="false"/>
      </requestedPrivileges>
    </security>
  </trustInfo>
  <compatibility xmlns="urn:schemas-microsoft-com:compatibility.v1">
    <application>
      <supportedOS Id="{8e0f7a12-bfb3-4fe8-b9a5-48fd50a15a9a}"/>
    </application>
  </compatibility>
</assembly>"#,
            );
        }

        // Só no app: os exemplos (a `vitrine`) não levam ícone nem pedem administrador.
        resource.compile_for(&["unkvoid"]).expect("o ícone não entrou no .exe");
    }

    if std::env::var("CARGO_CFG_TARGET_ENV").as_deref() == Ok("msvc") {
        static_vcruntime();
    }
}

/// O `VCRUNTIME140.dll` dentro do `.exe`. Ele não vem com o Windows, e sim com o
/// Visual C++ Redistributable: no PC sem ele o Windows recusa abrir o app, sem log nenhum. O
/// Tauri, que este substitui, já embutia; é a mesma receita do `tauri-build`
/// (`static_vcruntime.rs`, de github.com/ChrisDenton/static_vcruntime). O UCRT segue do
/// sistema, que o tem desde o Windows 10. O Rust pede o `msvcrt.lib` por nome, e um vazio na
/// frente dele no caminho do linker o anula.
fn static_vcruntime() {
    let machine: &[u8] = match std::env::var("CARGO_CFG_TARGET_ARCH").as_deref() {
        Ok("x86_64") => &[0x64, 0x86],
        Ok("x86") => &[0x4C, 0x01],
        _ => return,
    };
    let empty_library: &[u8] = &[
        1, 0, 94, 3, 96, 98, 60, 0, 0, 0, 1, 0, 0, 0, 0, 0, 132, 1, 46, 100, 114, 101, 99, 116, 118, 101, 0, 0, 0, 0, 0, 0,
        0, 0, 0, 0, 0, 0, 60, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 10, 16, 0, 46, 100, 114, 101, 99, 116, 118,
        101, 0, 0, 0, 0, 1, 0, 0, 0, 3, 0, 4, 0, 0, 0,
    ];
    let out_dir = std::env::var("OUT_DIR").expect("o Cargo sempre passa o OUT_DIR");

    std::fs::write(std::path::Path::new(&out_dir).join("msvcrt.lib"), [machine, empty_library].concat())
        .expect("o msvcrt.lib vazio não foi gravado");
    println!("cargo:rustc-link-search=native={out_dir}");

    for library in ["libvcruntimed.lib", "vcruntime.lib", "vcruntimed.lib", "libcmtd.lib", "msvcrt.lib", "msvcrtd.lib", "libucrt.lib", "libucrtd.lib"] {
        println!("cargo:rustc-link-arg=/NODEFAULTLIB:{library}");
    }

    for library in ["libcmt.lib", "libvcruntime.lib", "ucrt.lib"] {
        println!("cargo:rustc-link-arg=/DEFAULTLIB:{library}");
    }
}

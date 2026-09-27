fn main() {
    slint_build::compile("ui/app.slint").expect("a interface não compilou");

    // O ícone de dentro do .exe: é o que o Explorer, o atalho e a barra de tarefas mostram. O
    // mesmo desenho do app do Tauri, que este substitui no mesmo lugar.
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        let mut resource = tauri_winres::WindowsResource::new();

        resource.set_icon("../desktop/src-tauri/icons/icon.ico");

        // Roda com o nível mais alto da conta por causa dos Clips: numa conta de administrador,
        // elevado; numa conta comum, sem elevação, e o app abre do mesmo jeito (só os atalhos
        // não alcançam jogos elevados). `requireAdministrator` travaria quem não é administrador
        // fora do app, sala por código incluída. O anti-cheat de vários jogos (o do Arena
        // Breakout, por exemplo) põe o jogo em nível elevado, e o Windows esconde o teclado e a
        // janela de um jogo elevado de programas comuns: o Alt+Z não chegava e o painel não subia
        // por cima dele. De quebra, o push-to-talk passa a funcionar com esses jogos na frente.
        //
        // Só no build de release, que é o que o instalador leva: o Cargo põe o recurso do
        // binário também no executável dos testes dele, e aí todo `cargo test` pediria um UAC.
        if std::env::var("PROFILE").as_deref() == Ok("release") {
            resource.set_manifest(
                r#"<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <trustInfo xmlns="urn:schemas-microsoft-com:asm.v3">
    <security>
      <requestedPrivileges>
        <requestedExecutionLevel level="highestAvailable" uiAccess="false"/>
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
}

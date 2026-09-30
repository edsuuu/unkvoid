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
}

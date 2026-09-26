import Foundation
import Testing

@testable import Unkvoid

/// A entrada pelo caminho que o clique percorre: `AppModel`, e não o núcleo.
///
/// O núcleo já recusa os casos vazios — e mesmo assim dava para entrar, porque o botão
/// falava com o SFU em vez de falar com `unkvoid_app` e a tela trocava assim que a resposta
/// voltava, qualquer que fosse ela. Um teste no núcleo não pega isso; este pega.
@MainActor
struct EntryTests {
    /// O erro de uma tela não atravessa para a outra: o código recusado na entrada não pode
    /// continuar aceso quando a pessoa vai para o Hub e volta.
    @Test
    func anErrorNeverFollowsThePersonToAnotherScreen() async {
        let model = AppModel(url: "ws://127.0.0.1:1/sfu")

        model.name = "Ada"
        model.code = "-x-"

        await model.joinRoom()

        #expect(model.codeError != "")

        model.say("um aviso da entrada")
        model.complain("um erro de sala")
        model.openHub()

        #expect(model.codeError == "" && model.notice == nil && model.roomError == nil, "o erro da entrada apareceu no Hub")

        model.openEntry()

        #expect(model.codeError == "" && model.entryError == "")
    }

    /// O que o botão "Entrar" ao lado do campo de código faz, com o campo em branco de
    /// várias maneiras. Nenhuma delas pode abrir sala nenhuma.
    @Test(arguments: ["", "   ", "\t", "\n", "-x-"])
    func joiningWithoutARealCodeNeverOpensARoom(code: String) async {
        let model = AppModel(url: "ws://127.0.0.1:1/sfu")

        model.name = "Ada"
        model.code = code

        await model.joinRoom()

        #expect(model.screen != .room, "entrou numa sala com o código \(code.debugDescription)")
        #expect(model.room == nil)
        #expect(model.codeError != "", "recusou sem dizer nada no campo do código")
    }

    /// Sem nome não se entra, nem criando nem com código bom — e quem acende é o campo do
    /// nome, não o do código.
    @Test(arguments: ["", "   "])
    func enteringWithoutANameNeverOpensARoom(name: String) async {
        let creating = AppModel(url: "ws://127.0.0.1:1/sfu")

        creating.name = name
        creating.code = ""

        await creating.createRoom()

        #expect(creating.screen != .room, "criou uma sala sem nome")
        #expect(creating.nameError != "", "recusou sem dizer nada no campo do nome")
        #expect(creating.codeError == "", "acendeu o campo errado")

        let joining = AppModel(url: "ws://127.0.0.1:1/sfu")

        joining.name = name
        joining.code = "sala-de-teste"

        await joining.joinRoom()

        #expect(joining.screen != .room, "entrou numa sala sem nome")
        #expect(joining.nameError != "", "recusou sem dizer nada no campo do nome")
    }

    /// O erro tem de acender o campo que errou, e só ele. Pintar os dois, ou pendurar o
    /// texto do e-mail embaixo da senha, esconde exatamente o que a pessoa precisa saber.
    @Test(.enabled(if: EndToEndTests.stackIsUp), arguments: [
        ("", "qualquer-coisa", true),
        ("ada@teste.local", "", false),
    ])
    func aLoginErrorLightsUpOnlyTheFieldThatFailed(email: String, password: String, blamesEmail: Bool) async {
        let model = AppModel(url: "ws://127.0.0.1:1/sfu")

        model.email = email
        model.password = password

        await model.signIn(registering: false)

        #expect(model.screen != .hub, "entrou com \(email.debugDescription) e \(password.debugDescription)")

        if blamesEmail {
            #expect(model.emailError != "", "o e-mail errou e não acendeu")
            #expect(model.passwordError == "", "acendeu a senha por um erro do e-mail")
        } else {
            #expect(model.passwordError != "", "a senha errou e não acendeu")
            #expect(model.emailError == "", "acendeu o e-mail por um erro da senha")
        }

        let shown = model.emailError + model.passwordError

        #expect(!shown.contains("http") && !shown.contains("422"))
    }

    /// Criar sem código **sorteia** um: é o produto, e não um furo de validação. Se isto
    /// quebrar, a regra mudou no núcleo e alguém precisa saber.
    @Test(.enabled(if: EndToEndTests.stackIsUp))
    func creatingWithoutACodeDrawsOne() async {
        #expect(EndToEndTests.isolated)

        let model = AppModel(url: Launch.socketUrl())

        // Criar uma sala é entrar nela de verdade, e para isso o SFU tem de estar ligado.
        await model.start()

        model.name = "Ada"
        model.code = ""

        await model.createRoom()

        #expect(model.screen == .room, "não entrou: \(model.entryError) \(model.offlineStatus)")
        #expect(model.room?.isEmpty == false, "abriu a sala sem código nenhum")
        #expect(model.nameError == "" && model.codeError == "")

        await model.leaveRoom()
    }

    /// O token vence (ou é revogado noutro aparelho) com o app aberto: a próxima chamada
    /// devolve à entrada, com o aviso, em vez de deixar o hub na tela sem conta.
    @Test(.enabled(if: HubTests.ready))
    func anExpiredTokenLandsOnTheEntryScreen() async throws {
        #expect(EndToEndTests.isolated)

        let model = AppModel(url: Launch.socketUrl())

        await model.start()

        model.email = "grace@teste.local"
        model.password = ProcessInfo.processInfo.environment["UNKVOID_TEST_PASSWORD"] ?? ""

        await model.signIn(registering: false)

        try #require(model.signedIn, "a Grace não entrou: \(model.loginError)")
        #expect(model.screen == .hub)

        // Revoga o token no servidor sem o núcleo saber: é o que "vencer" parece daqui.
        #expect(await model.ask("api", ["name": "signOut", "params": [:], "body": [:]])["failed"] == nil)

        await model.loadServers()

        #expect(model.screen == .entry)
        #expect(!model.signedIn)
        #expect(model.user == nil)
        #expect(model.notice == "Sua sessão expirou. Entre de novo.")
    }
}

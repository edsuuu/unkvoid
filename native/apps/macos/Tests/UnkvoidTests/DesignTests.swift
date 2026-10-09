import SwiftUI
import Testing

@testable import Unkvoid

/// O desenho que não se vê num teste de tela: os ícones vêm de strings copiadas do
/// `Icon.tsx` e passam por um leitor de SVG feito à mão. Se ele deixar de entender um
/// comando, o ícone some ou vaza da caixa — e é exatamente isso que se verifica aqui.
struct DesignTests {
    @Test
    func everyIconDrawsSomethingInsideItsBox() {
        for name in IconName.allCases {
            let parts = Icon.parts(of: name)

            #expect(!parts.isEmpty, "\(name) não tem desenho nenhum")

            for part in parts {
                let box = IconShape(part: part).path(in: CGRect(x: 0, y: 0, width: 24, height: 24)).boundingRect

                #expect(!box.isEmpty, "\(name) tem uma parte vazia: o leitor não entendeu o comando")
                // Meio ponto de folga: a curva de um arco passa raspando na borda.
                #expect(box.minX >= -0.5 && box.minY >= -0.5 && box.maxX <= 24.5 && box.maxY <= 24.5, "\(name) vazou da caixa: \(box)")
            }
        }
    }

    /// Os pedaços de SVG que o `Icon.tsx` usa, um por um. Um `h` lido como `H` desenha o
    /// traço no lugar errado e ninguém percebe até olhar a tela.
    @Test
    func theReaderUnderstandsEveryCommandTheIconsUse() {
        #expect(SvgPath.parse("M5 7h14").boundingRect == CGRect(x: 5, y: 7, width: 14, height: 0))
        #expect(SvgPath.parse("M4 4l16 16").boundingRect == CGRect(x: 4, y: 4, width: 16, height: 16))
        #expect(SvgPath.parse("M12 5v14").boundingRect == CGRect(x: 12, y: 5, width: 0, height: 14))
        #expect(SvgPath.parse("M19 12H5").boundingRect == CGRect(x: 5, y: 12, width: 14, height: 0))

        // Um `z` volta para onde o traço começou, e o `M` seguinte recomeça de lá.
        #expect(SvgPath.parse("M4 5h16v11H9l-4 4v-4H4z").boundingRect == CGRect(x: 4, y: 5, width: 16, height: 15))

        let arc = SvgPath.parse("M5 11a7 7 0 0 0 14 0").boundingRect

        #expect(abs(arc.minX - 5) < 0.1 && abs(arc.maxX - 19) < 0.1, "o arco não foi de ponta a ponta: \(arc)")
        #expect(abs(arc.maxY - 18) < 0.1, "o arco não desceu o raio inteiro: \(arc)")
    }

    /// Um número colado no outro (`1.5.5`, `-.8`) é como o `Icon.tsx` escreve: sem isso o
    /// `gear` e o `phoneOff` saem tortos.
    @Test
    func gluedNumbersAreReadAsTwo() {
        let glued = SvgPath.parse("M0 0l1.5.5").boundingRect

        #expect(abs(glued.width - 1.5) < 0.01 && abs(glued.height - 0.5) < 0.01, "leu \(glued)")

        let negative = SvgPath.parse("M2 2l-.8-.8").boundingRect

        #expect(abs(negative.minX - 1.2) < 0.01 && abs(negative.width - 0.8) < 0.01, "leu \(negative)")
    }

    /// O "G" do Google são quatro caminhos copiados do `AuthCard.tsx`, num `viewBox` de 48.
    /// Se o leitor engasgar num deles, o botão de entrar mostra meia marca.
    @Test
    func theGoogleMarkFillsItsBox() {
        let ring = VectorPath(
            commands: "M43.6 20.5H42V20H24v8h11.3C33.7 32.7 29.2 36 24 36c-6.6 0-12-5.4-12-12s5.4-12 12-12c3 0 5.8 1.1 7.9 3l5.7-5.7C34 6.1 29.3 4 24 4 12.9 4 4 12.9 4 24s8.9 20 20 20 20-8.9 20-20c0-1.3-.1-2.4-.4-3.5z",
            viewBox: 48
        ).path(in: CGRect(x: 0, y: 0, width: 48, height: 48)).boundingRect

        #expect(abs(ring.minX - 4) < 0.5 && abs(ring.minY - 4) < 0.5, "o anel não começa onde devia: \(ring)")
        #expect(abs(ring.maxX - 44) < 0.5 && abs(ring.maxY - 44) < 0.5, "o anel não termina onde devia: \(ring)")
    }

    /// A cor de um cargo vem do Laravel como texto; o que não for `#rrggbb` não vira cor
    /// nenhuma, em vez de virar preto.
    @Test
    func onlyASixDigitHexBecomesAColor() {
        #expect(Theme.hex("#9a9cff") != nil)
        #expect(Theme.hex(nil) == nil)
        #expect(Theme.hex("vermelho") == nil)
        #expect(Theme.hex("#abc") == nil)
    }

    /// Cada token tem um valor por tema, e são os da nota: o centro é `#313338` no escuro e
    /// branco no claro; o destaque é o mesmo violeta nos dois. Se o `NSColor` dinâmico deixar
    /// de seguir a aparência, o tema claro inteiro sai escuro sem ninguém notar.
    @Test
    func theTokensFollowTheAppearance() {
        #expect(Self.resolved(Theme.surfaceChat, dark: true) == "#313338")
        #expect(Self.resolved(Theme.surfaceChat, dark: false) == "#ffffff")
        #expect(Self.resolved(Theme.surfaceRail, dark: true) == "#1e1f22")
        #expect(Self.resolved(Theme.surfaceRail, dark: false) == "#e3e5e8")
        #expect(Self.resolved(Theme.inkStrong, dark: true) == "#f2f3f5")
        #expect(Self.resolved(Theme.inkStrong, dark: false) == "#060607")
        #expect(Self.resolved(Theme.brand, dark: true) == "#6a55e0")
        #expect(Self.resolved(Theme.brand, dark: false) == "#6a55e0")
    }

    /// A grade da chamada: `ceil(sqrt(n))` colunas, e o cartão cabe na largura e na altura.
    @Test
    func theCallGridHasAsManyColumnsAsTheSquareRoot() {
        #expect(CallLayout.columns(for: 0) == 1)
        #expect(CallLayout.columns(for: 1) == 1)
        #expect(CallLayout.columns(for: 2) == 2)
        #expect(CallLayout.columns(for: 4) == 2)
        #expect(CallLayout.columns(for: 5) == 3)
        #expect(CallLayout.columns(for: 9) == 3)
        #expect(CallLayout.columns(for: 10) == 4)

        // Dois cartões numa caixa larga: a altura é o que limita, e o cartão fica 16:9 dentro dela.
        #expect(CallLayout.cardWidth(in: CGSize(width: 2000, height: 180), count: 2) == 320)
        // Quatro cartões numa caixa estreita: a largura é o que limita, e cada um leva metade menos o vão.
        #expect(CallLayout.cardWidth(in: CGSize(width: 408, height: 2000), count: 4) == 200)
    }

    /// As telas principais do Hub viram PNG numa pasta (`UNKVOID_SNAPSHOT_DIR`), com um servidor
    /// de exemplo e sem rede: é a foto que vai para a nota quando a pilha local não está no ar.
    /// Sem a variável, pulado — não é uma verificação, é uma ferramenta.
    @Test(.enabled(if: ProcessInfo.processInfo.environment["UNKVOID_SNAPSHOT_DIR"] != nil))
    @MainActor
    func theHubRendersToAPictureForTheNote() async throws {
        let folder = URL(fileURLWithPath: ProcessInfo.processInfo.environment["UNKVOID_SNAPSHOT_DIR"]!)

        try FileManager.default.createDirectory(at: folder, withIntermediateDirectories: true)

        let model = AppModel(url: "ws://127.0.0.1:1/sfu")
        let general = Channel(id: "t1", name: "geral", type: "text", topic: "Conversa do estúdio, sem pressa.", position: 0, permissions: 0x3FFFF, user_limit: nil, overwrites: [])
        let rules = Channel(id: "t2", name: "regras", type: "text", topic: nil, position: 1, permissions: 0x3FFFF, user_limit: nil, overwrites: [])
        let meeting = Channel(id: "v1", name: "Reunião", type: "voice", topic: nil, position: 2, permissions: 0x3FFFF, user_limit: 5, overwrites: [])
        let gaming = Channel(id: "v2", name: "Jogatina", type: "voice", topic: nil, position: 3, permissions: 0x3FFFF, user_limit: nil, overwrites: [])
        let work = Channel(id: "c1", name: "Trabalho", type: "category", topic: nil, position: 4, permissions: 0x3FFFF, user_limit: nil, overwrites: [])
        let standup = Channel(id: "t3", name: "daily", type: "text", topic: nil, position: 5, permissions: 0x3FFFF, user_limit: nil, overwrites: [], parent_id: "c1")
        let focus = Channel(id: "v3", name: "Foco", type: "voice", topic: nil, position: 6, permissions: 0x3FFFF, user_limit: nil, overwrites: [], parent_id: "c1")
        let moderator = Role(id: 1, name: "Moderação", color: "#f0b232", position: 1, permissions: 0, is_everyone: false)
        let everyone = Role(id: 2, name: "@everyone", color: nil, position: 0, permissions: 0, is_everyone: true)
        let members = [
            Member(user_id: 1, name: "Ada", avatar_url: nil, nickname: nil, role_ids: [], server_mute: false, server_deaf: false, is_owner: true),
            Member(user_id: 2, name: "Grace", avatar_url: nil, nickname: nil, role_ids: [1], server_mute: false, server_deaf: false, is_owner: false),
            Member(user_id: 3, name: "Linus", avatar_url: nil, nickname: "Tux", role_ids: [], server_mute: true, server_deaf: false, is_owner: false),
            Member(user_id: 4, name: "Barbara", avatar_url: nil, nickname: nil, role_ids: [], server_mute: false, server_deaf: false, is_owner: false),
        ]

        model.user = User(id: 1, name: "Ada", email: "ada@teste.local", avatar_url: nil, avatar_uploaded: false, nickname_confirmed: true)
        model.servers = [
            ServerSummary(id: 1, name: "Estúdio Unkvoid", owner_id: 1, icon_url: nil, last_accessed_at: nil),
            ServerSummary(id: 2, name: "Jogos de sexta", owner_id: 2, icon_url: nil, last_accessed_at: nil),
        ]
        model.tree = ServerTree(
            id: 1, name: "Estúdio Unkvoid", owner_id: 1, invite_code: "abcd1234efgh", icon_url: nil,
            me: Membership(user_id: 1, permissions: 0x3FFFF), roles: [moderator, everyone],
            channels: [general, rules, meeting, gaming, work, standup, focus], members: members,
            voice: ["v1": [
                VoicePerson(user_id: 2, name: "Grace", sources: ["screen", "mic"], muted: false),
                VoicePerson(user_id: 3, name: "Linus", sources: ["mic"], muted: true),
            ]],
            bans: nil
        )
        model.abilities = Abilities(can: ["manageChannels", "createInvite", "manageServer", "manageRoles", "moveMembers"], owner: true, members: [:], roles: [])
        model.online = ["1", "2", "3"]
        model.home = false

        await model.chat.open(general)
        try render(model, to: folder.appendingPathComponent("hub-chat.png"))

        model.voiceChannel = meeting
        model.stageOpen = true
        model.voiceChatOpen = true
        try render(model, to: folder.appendingPathComponent("hub-chamada.png"))
    }

    /// Uma janela de verdade, fora da tela, no tema escuro: o `ImageRenderer` não desenha o que
    /// está dentro de um `ScrollView` nem o campo de texto, e cai na aparência do processo.
    @MainActor
    private func render(_ model: AppModel, to file: URL) throws {
        let host = NSHostingView(rootView: HubScreen().environmentObject(model))
        let window = NSWindow(contentRect: CGRect(x: 0, y: 0, width: 1280, height: 800), styleMask: .borderless, backing: .buffered, defer: false)

        window.appearance = NSAppearance(named: .darkAqua)
        window.contentView = host
        host.frame = window.contentView!.bounds
        host.layoutSubtreeIfNeeded()
        window.orderFront(nil)
        window.displayIfNeeded()

        let bitmap = try #require(host.bitmapImageRepForCachingDisplay(in: host.bounds))

        host.cacheDisplay(in: host.bounds, to: bitmap)
        window.orderOut(nil)

        let png = try #require(bitmap.representation(using: .png, properties: [:]))

        try png.write(to: file)
    }

    private static func resolved(_ color: Color, dark: Bool) -> String {
        var text = ""

        NSAppearance(named: dark ? .darkAqua : .aqua)!.performAsCurrentDrawingAppearance {
            text = color.hexText
        }

        return text
    }
}

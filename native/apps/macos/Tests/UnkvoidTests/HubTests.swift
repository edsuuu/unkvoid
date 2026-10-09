import AVFoundation
import Foundation
import Testing

@testable import Unkvoid

/// O Hub pelo caminho que o clique percorre: `AppModel` e `ChatRoom`, contra a pilha local.
/// Pulado sem servidor ou sem a senha das contas de teste (`UNKVOID_TEST_PASSWORD`).
@MainActor
@Suite(.serialized)
struct HubTests {
    private nonisolated static let password = ProcessInfo.processInfo.environment["UNKVOID_TEST_PASSWORD"] ?? ""

    nonisolated static var ready: Bool {
        EndToEndTests.stackIsUp && !password.isEmpty
    }

    /// Entra como a Ada e abre o servidor que ela divide com a Grace.
    ///
    /// Uma janela para a suíte inteira: o Laravel limita o login por minuto.
    private func signedIn() async throws -> AppModel {
        #expect(EndToEndTests.isolated)

        if let kept = Self.kept {
            return kept
        }

        let model = AppModel(url: Launch.socketUrl())

        await model.start()

        model.email = "ada@teste.local"
        model.password = Self.password

        await model.signIn(registering: false)

        try #require(model.signedIn, "a Ada não entrou: \(model.loginError)")

        let shared = try #require(model.servers.first { $0.name == "Estúdio Unkvoid" } ?? model.servers.first)

        await model.openServer(shared.id)

        Self.kept = model

        return model
    }

    private static var kept: AppModel?

    /// Sair da voz com mic e câmera ligados: o núcleo ainda anuncia `room.mine` com a câmera
    /// ligada enquanto recolhe o que subia. Com a sala fechada deste lado, isso não religa nada —
    /// a luz verde não fica acesa fora da chamada.
    @Test
    func aRoomMineAfterLeavingDoesNotTurnTheCameraBackOn() async {
        #expect(EndToEndTests.isolated)

        let model = AppModel(url: "ws://127.0.0.1:1/sfu")

        model.voiceChannel = Channel(id: "v1", name: "Reunião", type: "voice", topic: nil, position: 0, permissions: 0, user_limit: nil, overwrites: [])

        await model.leaveVoice()

        model.heard(["event": "room.mine", "data": ["camera": true, "mic": true]])

        #expect(!model.mine.camera)
        #expect(!model.cameraSync.starting)
        #expect(model.media?.camera.isRunning == false)
    }

    /// Entrar numa sala por código estando numa voz solta a voz antes: o núcleo já saía dela,
    /// mas o desenho, o chat da voz e o microfone ficavam vivos deste lado.
    @Test
    func enteringARoomByCodeLeavesTheVoiceFirst() async {
        #expect(EndToEndTests.isolated)

        let model = AppModel(url: "ws://127.0.0.1:1/sfu")

        model.voiceChannel = Channel(id: "v1", name: "Reunião", type: "voice", topic: nil, position: 0, permissions: 0, user_limit: nil, overwrites: [])
        model.stageOpen = true
        model.voiceChatOpen = true
        model.name = "Ada"
        model.code = "abcdefghijkl"

        await model.joinRoom()

        #expect(model.voiceChannel == nil)
        #expect(!model.stageOpen)
        #expect(!model.voiceChatOpen)
    }

    /// `channel.<ulid>` é a sala de um canal no SFU; o movido precisa do id do canal para pedir
    /// o token do destino.
    @Test
    func theRoomOfAVoiceChannelNamesTheChannel() {
        #expect(AppModel.channelId(ofRoom: "channel.01HZXABC") == "01HZXABC")
        #expect(AppModel.channelId(ofRoom: "abcdefghijkl") == "abcdefghijkl")
        #expect(AppModel.channelId(ofRoom: nil) == nil)
    }

    /// A mesma pessoa, logo depois, sem responder a ninguém: a mensagem continua a anterior e sai
    /// sem foto nem nome. Qualquer coisa diferente recomeça o grupo.
    @Test
    func aMessageContinuesThePreviousOneOnlyFromTheSamePersonSoonAfter() {
        let ada = Person(id: 1, name: "Ada", avatar_url: nil)
        let grace = Person(id: 2, name: "Grace", avatar_url: nil)
        let first = message(1, from: ada, at: "2026-10-09T10:00:00Z")

        #expect(ChatPanel.continues(message(2, from: ada, at: "2026-10-09T10:03:00Z"), after: first))
        #expect(!ChatPanel.continues(message(3, from: ada, at: "2026-10-09T10:08:00Z"), after: first))
        #expect(!ChatPanel.continues(message(4, from: grace, at: "2026-10-09T10:01:00Z"), after: first))
        #expect(!ChatPanel.continues(message(5, from: ada, at: "2026-10-09T10:01:00Z", replyTo: ReplyTo(id: 1, name: "Ada", body: "oi")), after: first))
        #expect(!ChatPanel.continues(message(6, from: ada, at: "2026-10-09T10:01:00Z", type: "join"), after: first))
    }

    private func message(_ id: Int, from person: Person, at when: String, replyTo: ReplyTo? = nil, type: String = "user") -> Message {
        Message(id: id, channel_id: "c", type: type, user: person, reply_to: replyTo, files: nil, body: "x", edited_at: nil, created_at: when)
    }

    @Test(.enabled(if: ready))
    func aMessageIsSentAnsweredEditedAndDeleted() async throws {
        let model = try await signedIn()
        let chat = model.chat

        try #require(chat.channel != nil, "o servidor abriu sem canal de texto")
        #expect(chat.canSend)

        let body = "do teste \(UUID().uuidString.prefix(8))"

        #expect(await chat.send(body))

        let sent = try #require(chat.messages.last { $0.body == body })

        #expect(chat.isMine(sent) && chat.canDelete(sent))

        chat.replyTo = sent

        #expect(await chat.send("resposta"))

        let answer = try #require(chat.messages.last)

        #expect(answer.reply_to?.id == sent.id, "a resposta perdeu a quem respondia")
        #expect(chat.replyTo == nil, "a resposta enviada continua marcada")

        #expect(await chat.edit(answer, to: "resposta editada"))
        #expect(chat.messages.last?.body == "resposta editada")
        #expect(chat.messages.last?.edited_at != nil)

        await chat.delete(answer)
        await chat.delete(sent)

        #expect(!chat.messages.contains { $0.id == sent.id || $0.id == answer.id })
    }

    /// O que a pessoa pode fazer vem do núcleo junto da árvore, e a dona pode tudo.
    @Test(.enabled(if: ready))
    func theOwnerSeesEveryTabAndActsOnOtherMembers() async throws {
        let model = try await signedIn()
        let tree = try #require(model.tree)

        #expect(model.abilities.owner == (tree.owner_id == model.user?.id))

        if model.abilities.owner {
            #expect(model.abilities.allows("manageRoles") && model.abilities.allows("banMembers"))

            let other = try #require(tree.members.first { $0.user_id != model.user?.id })

            #expect(model.abilities.actions(on: other).kick)
            #expect(!model.abilities.actions(on: try #require(tree.members.first { $0.user_id == model.user?.id })).kick, "ninguém se expulsa")
        }
    }

    /// As preferências sobrevivem a fechar o app: o núcleo as grava na pasta de estado.
    @Test(.enabled(if: EndToEndTests.stackIsUp))
    func aPreferenceIsStillThereInTheNextLaunch() async {
        #expect(EndToEndTests.isolated)

        let first = AppModel(url: Launch.socketUrl())

        await first.start()

        first.setVoice { $0.sensitivity = 61; $0.talk = "KeyV" }

        try? await Task.sleep(for: .milliseconds(200))

        let second = AppModel(url: Launch.socketUrl())

        await second.start()

        #expect(second.voicePreferences.sensitivity == 61)
        #expect(second.voicePreferences.talk == "KeyV")

        second.setVoice { $0 = VoicePreferences() }

        try? await Task.sleep(for: .milliseconds(200))
    }

    /// O toque chega do núcleo pela ABI: tem som de verdade, nunca estoura o volume, e começa
    /// em silêncio — senão faria "clique" no fone. Nome que não existe não toca nada.
    @Test
    func aChimeComesFromTheCoreAudibleAndClickFree() throws {
        let samples = try #require(Core.chime("joined"))
        let floats = samples.withUnsafeBytes { Array($0.bindMemory(to: Float.self)) }
        let peak = floats.map(abs).max() ?? 0

        #expect(floats.count == 9_120 * 2)
        #expect(peak > 0.03 && peak <= 0.071, "pico de \(peak)")
        #expect(abs(floats[0]) < 0.001)
        #expect(Core.chime("inexistente") == nil)
    }
}

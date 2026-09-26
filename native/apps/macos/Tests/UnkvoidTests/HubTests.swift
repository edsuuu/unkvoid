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

    /// Os toques são os do React, sintetizados: um toque tem som de verdade, nunca estoura o
    /// volume dele, e começa e termina em silêncio — senão faria "clique" no fone.
    @Test @MainActor
    func aChimeIsAudibleSoftAndClickFree() throws {
        let buffer = try #require(Sounds.render([.init(hertz: 523, startsAt: 0), .init(hertz: 784, startsAt: 0.08)]))
        let samples = UnsafeBufferPointer(start: buffer.floatChannelData?[0], count: Int(buffer.frameLength))
        let peak = samples.map(abs).max() ?? 0

        #expect(peak > 0.03 && peak <= Sounds.volume + 0.001, "pico de \(peak)")
        #expect(abs(samples.first ?? 1) < 0.001 && abs(samples.last ?? 1) < 0.002)
        #expect(buffer.frameLength == AVAudioFrameCount((0.08 + 0.09 + 0.02) * 48_000))
    }
}

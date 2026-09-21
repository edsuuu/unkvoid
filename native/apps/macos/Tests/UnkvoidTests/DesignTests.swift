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
}

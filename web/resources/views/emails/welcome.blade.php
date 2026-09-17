<x-mail.layout title="Bem-vindo ao Unkvoid">
    <x-mail.title eyebrow="Conta criada">Bem-vindo, {{ $name }}.</x-mail.title>
    <x-mail.text>Sua conta no Unkvoid está pronta. Com ela você cria salas e servidores, entra na voz e guarda seus clipes.</x-mail.text>
    <x-mail.text>Para compartilhar a tela, abra o app, crie uma sala e mande o código. Quem for assistir entra com a própria conta.</x-mail.text>
    <x-mail.button :href="$downloadUrl">Baixar o app</x-mail.button>
    <x-mail.text>Se você não criou esta conta, responda este e-mail que a gente apaga.</x-mail.text>
</x-mail.layout>

<x-mail.layout title="Bem-vindo ao Unkvoid">
    <x-mail.title eyebrow="Conta criada">Bem-vindo, {{ $name }}.</x-mail.title>
    <x-mail.text>Sua conta no Unkvoid está pronta. Ela guarda seu nome e suas preferências, e permite criar e participar de servidores.</x-mail.text>
    <x-mail.text>Abra o app, crie ou entre em um servidor e use os canais de voz para conversar e compartilhar a tela.</x-mail.text>
    <x-mail.button :href="$downloadUrl">Baixar o app</x-mail.button>
    <x-mail.text>Se você não criou esta conta, responda este e-mail que a gente apaga.</x-mail.text>
</x-mail.layout>

<x-mail.layout title="Bem-vindo ao Unkvoid">
    <x-mail.title eyebrow="Conta criada">Bem-vindo, {{ $name }}.</x-mail.title>
    <x-mail.text>Sua conta no Unkvoid está pronta. Ela serve para guardar seu nome e suas preferências, e para as salas que você criar continuarem suas depois de fechar o app.</x-mail.text>
    <x-mail.text>Compartilhar a tela continua igual: abra o app, crie uma sala e mande o código para quem você quiser.</x-mail.text>
    <x-mail.button :href="$downloadUrl">Baixar o app</x-mail.button>
    <x-mail.text>Se você não criou esta conta, responda este e-mail que a gente apaga.</x-mail.text>
</x-mail.layout>

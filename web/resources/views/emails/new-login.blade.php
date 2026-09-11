<x-mail.layout title="Novo acesso à sua conta">
    <x-mail.title eyebrow="Segurança">Novo acesso à sua conta.</x-mail.title>
    <x-mail.text>Alguém entrou na sua conta do Unkvoid agora há pouco. Se foi você, não precisa fazer nada.</x-mail.text>
    <x-mail.detail :rows="$rows" />
    <x-mail.text>Se não foi você, troque a senha e responda este e-mail.</x-mail.text>
    <x-mail.button :href="$resetUrl">Trocar a senha</x-mail.button>
</x-mail.layout>

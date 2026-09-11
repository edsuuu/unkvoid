<x-mail.layout title="Redefinir a senha">
    <x-mail.title eyebrow="Senha">Redefinir a sua senha.</x-mail.title>
    <x-mail.text>Recebemos um pedido para trocar a senha da sua conta. O link abaixo vale por {{ $minutes }} minutos.</x-mail.text>
    <x-mail.button :href="$url">Escolher uma senha nova</x-mail.button>
    <x-mail.text>Se você não pediu isso, ignore este e-mail. A senha continua a mesma.</x-mail.text>
</x-mail.layout>

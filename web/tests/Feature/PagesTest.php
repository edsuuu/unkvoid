<?php

declare(strict_types=1);

it('abre a página inicial com o download', function (): void {
    $this->get(route('home'))
        ->assertOk()
        ->assertSee('Baixar Unkvoid')
        ->assertSee('apt install unkvoid');
});

it('abre a política de privacidade', function (): void {
    $this->get(route('privacy'))
        ->assertOk()
        ->assertSee('Política de privacidade');
});

it('abre os termos de uso', function (): void {
    $this->get(route('terms'))
        ->assertOk()
        ->assertSee('Termos de uso');
});

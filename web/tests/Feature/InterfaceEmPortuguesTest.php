<?php

declare(strict_types=1);

/**
 * Código em inglês, interface em português.
 *
 * Uma tradução automática já levou junto o que o usuário lê, e o site inteiro virou
 * inglês sem ninguém notar: as chaves do `__()` mudaram e o `pt_BR.json` ficou para
 * trás, então cada chave caía no próprio nome. Este teste falha quando isso se repete.
 */
it('has a Portuguese translation for every string shown to the user', function (): void {
    $arquivos = array_merge(
        glob(resource_path('views/**/*.blade.php')) ?: [],
        glob(resource_path('views/**/**/*.blade.php')) ?: [],
        glob(app_path('**/*.php')) ?: [],
        glob(app_path('**/**/*.php')) ?: [],
    );

    $chaves = [];

    foreach ($arquivos as $arquivo) {
        preg_match_all("/__\('((?:[^'\\\\]|\\\\.)*)'/", (string) file_get_contents($arquivo), $achados);

        foreach ($achados[1] as $chave) {
            $chaves[] = str_replace("\\'", "'", $chave);
        }
    }

    $traduzidas = json_decode((string) file_get_contents(lang_path('pt_BR.json')), true);
    $doFramework = fn (string $chave): bool => str_contains($chave, '.') && ! str_contains($chave, ' ');

    $faltando = array_values(array_filter(
        array_unique($chaves),
        fn (string $chave): bool => ! isset($traduzidas[$chave]) && ! $doFramework($chave),
    ));

    expect($faltando)->toBe([], 'sem tradução em pt_BR.json: '.implode(' · ', $faltando));
});

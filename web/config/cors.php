<?php

declare(strict_types=1);

return [

    /*
    |--------------------------------------------------------------------------
    | Cross-Origin Resource Sharing (CORS) Configuration
    |--------------------------------------------------------------------------
    |
    | Here you may configure your settings for cross-origin resource sharing
    | or "CORS". This determines what cross-origin operations may execute
    | in web browsers. You are free to adjust these settings as needed.
    |
    | To learn more: https://developer.mozilla.org/en-US/docs/Web/HTTP/CORS
    |
    */

    'paths' => ['api/*', 'health', 'sanctum/csrf-cookie'],

    'allowed_methods' => ['*'],

    // Sem CORS_URL vale '*', como antes: o app instalado na máquina de quem usa não tem
    // uma origem fixa que dê para listar. Defina a variável só se souber quais são.
    'allowed_origins' => array_map(trim(...), explode(',', (string) env('CORS_URL', '*'))),

    'allowed_origins_patterns' => [],

    'allowed_headers' => ['*'],

    'exposed_headers' => [],

    'max_age' => 0,

    'supports_credentials' => false,

];

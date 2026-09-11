<?php

declare(strict_types=1);

return [

    'admin_email' => (string) env('UNKVOID_ADMIN_EMAIL', ''),

    'release_secret' => (string) env('RELEASE_SECRET', ''),

    'apt_url' => (string) env('UNKVOID_APT_URL', 'https://unkvoid.com/apt'),

];

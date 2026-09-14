<?php

declare(strict_types=1);

use Rector\Caching\ValueObject\Storage\FileCacheStorage;
use Rector\CodingStyle\Rector\Catch_\CatchExceptionNameMatchingTypeRector;
use Rector\CodingStyle\Rector\Encapsed\EncapsedStringsToSprintfRector;
use Rector\Config\RectorConfig;
use Rector\EarlyReturn\Rector\If_\ChangeOrIfContinueToMultiContinueRector;
use Rector\Exception\Configuration\InvalidConfigurationException;
use Rector\Php83\Rector\ClassMethod\AddOverrideAttributeToOverriddenMethodsRector;
use Rector\TypeDeclaration\Rector\StmtsAwareInterface\DeclareStrictTypesRector;
use RectorLaravel\Rector\Class_\AddHasFactoryToModelsRector;
use RectorLaravel\Set\LaravelSetList;
use RectorLaravel\Set\LaravelSetProvider;

try {
    return RectorConfig::configure()
        ->withPaths([
            __DIR__.'/app',
            __DIR__.'/config',
            __DIR__.'/database',
            __DIR__.'/routes',
            __DIR__.'/tests',
        ])
        ->withSetProviders(LaravelSetProvider::class)
        ->withImportNames(
            removeUnusedImports: true,
        )
        ->withCache(
            cacheDirectory: '/tmp/rector',
            cacheClass: FileCacheStorage::class,
        )
        ->withPhpSets(php84: true)
        ->withSets([
            LaravelSetList::LARAVEL_100,

            // Laravel
            LaravelSetList::LARAVEL_COLLECTION,
            LaravelSetList::LARAVEL_CODE_QUALITY,
            LaravelSetList::LARAVEL_IF_HELPERS,

            // Queries
            LaravelSetList::LARAVEL_ELOQUENT_MAGIC_METHOD_TO_QUERY_BUILDER,

            // Container
            LaravelSetList::LARAVEL_CONTAINER_STRING_TO_FULLY_QUALIFIED_NAME,

            // Facades
            LaravelSetList::LARAVEL_FACADE_ALIASES_TO_FULL_NAMES,

            // Factories
            LaravelSetList::LARAVEL_FACTORIES,
            LaravelSetList::LARAVEL_LEGACY_FACTORIES_TO_CLASSES,

            // Helpers modernos
            LaravelSetList::LARAVEL_ARRAY_STR_FUNCTION_TO_STATIC_CALL,
            LaravelSetList::LARAVEL_ARRAYACCESS_TO_METHOD_CALL,
        ])
        ->withSkip([
            AddOverrideAttributeToOverriddenMethodsRector::class,
            // O estilo do projeto: `$exception` no catch, interpolação em vez de sprintf,
            // HasFactory só em model que tem factory (sem o genérico o phpstan reclama), e
            // guards de mesmo desfecho num `if` só com `||`.
            AddHasFactoryToModelsRector::class,
            CatchExceptionNameMatchingTypeRector::class,
            ChangeOrIfContinueToMultiContinueRector::class,
            EncapsedStringsToSprintfRector::class,
        ])
        ->withPreparedSets(
            deadCode: true,
            codeQuality: true,
            codingStyle: true,
            typeDeclarations: true,
            privatization: true,
            earlyReturn: true,
        )
        ->withRules([
            DeclareStrictTypesRector::class,
        ]);
} catch (InvalidConfigurationException $e) {

}

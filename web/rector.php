<?php

declare(strict_types=1);

use Rector\Caching\ValueObject\Storage\FileCacheStorage;
use Rector\Config\RectorConfig;
use Rector\Exception\Configuration\InvalidConfigurationException;
use Rector\Php83\Rector\ClassMethod\AddOverrideAttributeToOverriddenMethodsRector;
use Rector\TypeDeclaration\Rector\StmtsAwareInterface\DeclareStrictTypesRector;
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

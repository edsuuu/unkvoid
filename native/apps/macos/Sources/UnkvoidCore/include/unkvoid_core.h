/*
 * A superfície do núcleo em Rust, como o Swift a enxerga.
 *
 * Toda string devolvida daqui foi alocada do lado do Rust e tem de voltar em
 * unkvoid_string_free — uma vez só. Ver shared/core/src/ffi.rs.
 */
#ifndef UNKVOID_CORE_H
#define UNKVOID_CORE_H

#include <stdbool.h>

typedef struct Handle Handle;

Handle *unkvoid_core_new(void);
void unkvoid_core_free(Handle *handle);

bool unkvoid_connect(Handle *handle, const char *url);

/* A resposta do SFU em JSON. Liberar com unkvoid_string_free.
 *
 * BLOQUEIA até o servidor responder: chamar da thread que desenha congela a janela. */
char *unkvoid_call(Handle *handle, const char *action, const char *data_json);

/* As decisões do app, que não passam pelo SFU: qual tela vale, entrar numa sala, entrar
 * na conta, os servidores, as mensagens. Devolve JSON com "ok", "failed" (um motivo
 * que a interface traduz) ou "invalid" (texto de validação, já em português).
 *
 * As ações que falam com o servidor também bloqueiam. */
char *unkvoid_app(Handle *handle, const char *action, const char *data_json);

/* O próximo evento da fila, ou NULL se não há nenhum. Não bloqueia. */
char *unkvoid_next_event(Handle *handle);

void unkvoid_string_free(char *text);

#endif

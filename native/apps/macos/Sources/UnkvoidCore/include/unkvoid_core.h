/*
 * A superfície do núcleo em Rust, como o Swift a enxerga.
 *
 * Toda string devolvida daqui foi alocada do lado do Rust e tem de voltar em
 * unkvoid_string_free — uma vez só. Ver shared/core/src/ffi.rs.
 */
#ifndef UNKVOID_CORE_H
#define UNKVOID_CORE_H

#include <stdbool.h>
#include <stddef.h>

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

/* O próximo quadro ou bloco de som do que se está assistindo. Espera até 100 ms e devolve
 * NULL se nada chegou: é para UMA thread só da interface, em laço. O bloco é um cabeçalho
 * e o conteúdo, tudo little-endian:
 *   [tipo u8: 0 vídeo, 1 som][keyframe u8][tamanho do id u16][timestamp u32][id][conteúdo]
 * Vídeo é H.264 em Annex-B; som é PCM f32 estéreo intercalado a 48 kHz.
 * Liberar com unkvoid_bytes_free, com o mesmo length. */
unsigned char *unkvoid_next_media(Handle *handle, size_t *length);

void unkvoid_bytes_free(unsigned char *block, size_t length);

/* O som do microfone que a interface captura: PCM f32 estéreo intercalado a 48 kHz.
 * Sem sala ou sem microfone aberto (ação "openMicrophone"), não faz nada. */
void unkvoid_speak(Handle *handle, const float *samples, size_t count);

/* Um quadro da câmera que a interface capturou: o IOSurfaceRef dele, JÁ RETIDO (+1) — quem
 * solta é o núcleo. Sem sala ou sem câmera aberta (ação "openCamera"), só solta. */
void unkvoid_show(Handle *handle, void *surface, unsigned long long timestamp_ns);

void unkvoid_string_free(char *text);

#endif

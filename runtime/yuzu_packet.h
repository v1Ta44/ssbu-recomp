/* Yuzu v1 wire encoding; based on packet.{cpp,h}, GPL-2.0-or-later,
 * Citra contributors (2017), yuzu-mainline 7ffac53c9e67. Bounded C implementation. */
#ifndef SSBU_YUZU_PACKET_H
#define SSBU_YUZU_PACKET_H
#include <stdint.h>
#include <stddef.h>
#include <string.h>
typedef struct { uint8_t *data; size_t size, pos; int bad; } YnWire;
static inline uint64_t yw_read(YnWire *w, unsigned n) {
    uint64_t v=0;
    if (n>8 || w->pos>w->size || n>w->size-w->pos) { w->bad=1; return 0; }
    while (n--) v=(v<<8)|w->data[w->pos++];
    return v;
}
static inline void yw_write(YnWire *w, uint64_t v, unsigned n) {
    if (n>8 || w->pos>w->size || n>w->size-w->pos) { w->bad=1; return; }
    for (unsigned i=n;i;i--) w->data[w->pos++]=(uint8_t)(v>>((i-1)*8));
}
static inline const uint8_t *yw_take(YnWire *w, size_t n) {
    if (w->pos>w->size || n>w->size-w->pos) { w->bad=1; return NULL; }
    const uint8_t *p=w->data+w->pos; w->pos+=n; return p;
}
static inline void yw_bytes(YnWire *w, const void *p, size_t n) {
    if (w->pos>w->size || n>w->size-w->pos || (!p && n)) { w->bad=1; return; }
    if (n) memcpy(w->data+w->pos,p,n); w->pos+=n;
}
static inline void yw_string(YnWire *w, const char *s) { size_t n=strlen(s); yw_write(w,n,4); yw_bytes(w,s,n); }
static inline void yw_get_string(YnWire *w, char *out, size_t cap) {
    size_t n=(size_t)yw_read(w,4); const uint8_t *p=yw_take(w,n);
    if (!cap) return;
    out[0]=0;
    if (!p) return;
    /* Bounded display copy, while consuming the entire upstream string. */
    size_t copy=n<cap-1?n:cap-1;
    if (copy<n) while (copy && (p[copy]&0xc0)==0x80) copy--;
    memcpy(out,p,copy); out[copy]=0;
}
#endif

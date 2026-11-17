#include <fcntl.h>
#include <stdlib.h>
#include <unistd.h>

#include "../include/context.h"
#include "internal.h"

AcerContext *acer_open(void)
{
    AcerContext *ctx = malloc(sizeof(*ctx));

    if (!ctx) {
        return NULL;
    }

    ctx->fd = open(ACERCONTROL_DEVICE, O_RDWR);

    if (ctx->fd < 0) {
        /* Stay silent: callers report the failure. A library must not
         * spam stderr, especially when polled every second for telemetry. */
        free(ctx);
        return NULL;
    }

    return ctx;
}

void acer_close(AcerContext *ctx)
{
    if (!ctx)
        return;

    if (ctx->fd >= 0)
        close(ctx->fd);

    free(ctx);
}

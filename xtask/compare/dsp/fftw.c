#include <fftw3.h>
#include <string.h>

#include "bench.h"

typedef struct {
    unsigned n;
    fftwf_complex *input;
    fftwf_complex *work;
    fftwf_complex *output;
    fftwf_plan plan;
} fft_ctx;

static void fft_step(void *raw) {
    fft_ctx *ctx = raw;
    memcpy(ctx->work, ctx->input, sizeof(fftwf_complex) * ctx->n);
    fftwf_execute(ctx->plan);
}

static int measure(const char *id, unsigned n, const bench_timing *timing) {
    fft_ctx ctx;
    ctx.n = n;
    ctx.input = fftwf_malloc(sizeof(fftwf_complex) * n);
    ctx.work = fftwf_malloc(sizeof(fftwf_complex) * n);
    ctx.output = fftwf_malloc(sizeof(fftwf_complex) * n);
    if (!ctx.input || !ctx.work || !ctx.output) return 1;
    ctx.plan = fftwf_plan_dft_1d((int)n, ctx.work, ctx.output, FFTW_FORWARD, FFTW_MEASURE);
    bench_signal((float *)ctx.input, n, 0xFF7);
    bench_run(id, fft_step, &ctx, n, timing);
    fftwf_destroy_plan(ctx.plan);
    fftwf_free(ctx.input);
    fftwf_free(ctx.work);
    fftwf_free(ctx.output);
    return 0;
}

int main(int argc, char **argv) {
    bench_timing timing;
    if (bench_parse(argc, argv, &timing)) return 1;
    const char *version = fftwf_version;
    if (strncmp(version, "fftw-", 5) == 0) version += 5;
    printf("version\t%s\n", version);
    return measure("fft", BENCH_FFT, &timing) || measure("fft-64k", BENCH_BIG_FFT, &timing) ||
           measure("fft-1m", BENCH_HUGE_FFT, &timing);
}

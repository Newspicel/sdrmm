#include <gnuradio/analog/quadrature_demod_cf.h>
#include <gnuradio/blocks/rotator.h>
#include <gnuradio/constants.h>
#include <gnuradio/fft/fft.h>
#include <gnuradio/filter/fft_filter.h>
#include <gnuradio/filter/fir_filter.h>
#include <gnuradio/filter/firdes.h>
#include <gnuradio/filter/freq_xlating_fir_filter.h>
#include <gnuradio/filter/pfb_arb_resampler.h>
#include <gnuradio/filter/polyphase_filterbank.h>
#include <gnuradio/logger.h>
#include <spdlog/sinks/dist_sink.h>
#include <spdlog/sinks/stdout_sinks.h>
#include <volk/volk_version.h>

#include <algorithm>
#include <complex>
#include <cstring>
#include <memory>
#include <string>
#include <vector>

#include "bench.h"

namespace {

constexpr double ddc_input_rate = 20e6;
constexpr double ddc_output_rate = 48e3;
constexpr double ddc_offset = 187500.0;
constexpr int ddc_decimation = 25;
constexpr double ddc_passband = 0.4 * ddc_output_rate;
constexpr double ddc_protect = 0.5 * ddc_output_rate;
constexpr double stopband_db = 60.0;
constexpr unsigned arb_filters = 32;

using complex = gr_complex;
using samples = std::vector<complex>;

struct history {
    samples buffer;
    std::size_t keep;

    explicit history(std::size_t keep) : buffer(keep, complex{}), keep(keep) {}

    void append(const complex* data, std::size_t n) { buffer.insert(buffer.end(), data, data + n); }

    void consume(std::size_t n) { buffer.erase(buffer.begin(), buffer.begin() + n); }
};

struct arb_stage {
    gr::filter::kernel::pfb_arb_resampler_ccf kernel;
    history input;
    samples output;
    unsigned long produced = 0;

    arb_stage(float rate, const std::vector<float>& taps)
        : kernel(rate, taps, arb_filters), input(kernel.taps_per_filter() - 1)
    {
    }

    void process(const complex* data, std::size_t n)
    {
        input.append(data, n);
        int available = int(input.buffer.size()) - int(kernel.taps_per_filter()) + 1;
        if (available <= 0) return;
        output.resize(std::size_t(available * kernel.fractional_rate() * kernel.interpolation_rate()) + 64);
        int consumed = 0;
        int made = kernel.filter(output.data(), input.buffer.data(), available, consumed);
        output.resize(std::size_t(made));
        produced += std::size_t(made);
        input.consume(std::size_t(consumed));
    }
};

struct xlating_stage {
    gr::filter::freq_xlating_fir_filter_ccf::sptr block;
    history input;
    samples output;
    unsigned ntaps;

    explicit xlating_stage(const std::vector<float>& taps)
        : block(gr::filter::freq_xlating_fir_filter_ccf::make(
              ddc_decimation, taps, ddc_offset, ddc_input_rate)),
          input(taps.size() - 1),
          ntaps(unsigned(taps.size()))
    {
    }

    void process(const complex* data, std::size_t n)
    {
        input.append(data, n);
        std::size_t usable = input.buffer.size() - (ntaps - 1);
        int produced = int(usable / ddc_decimation);
        output.resize(std::size_t(produced));
        gr_vector_const_void_star in{ input.buffer.data() };
        gr_vector_void_star out{ output.data() };
        int done = block->work(produced, in, out);
        output.resize(std::size_t(done));
        input.consume(std::size_t(done) * ddc_decimation);
    }
};

struct channel_bank : gr::filter::kernel::polyphase_filterbank {
    std::vector<samples> arms;
    std::size_t frames = 0;

    explicit channel_bank(const std::vector<float>& taps)
        : polyphase_filterbank(BENCH_CHANNELS, taps),
          arms(BENCH_CHANNELS, samples(d_taps_per_filter, complex{}))
    {
    }

    void deinterleave(const complex* data, std::size_t n)
    {
        frames = n / BENCH_CHANNELS;
        for (unsigned arm = 0; arm < BENCH_CHANNELS; arm++) {
            auto& stream = arms[arm];
            stream.erase(stream.begin(), stream.end() - (d_taps_per_filter - 1));
            for (std::size_t frame = 0; frame < frames; frame++)
                stream.push_back(data[frame * BENCH_CHANNELS + arm]);
        }
    }

    void process(const complex* data, std::size_t n, complex* out)
    {
        deinterleave(data, n);
        for (std::size_t frame = 0; frame < frames; frame++) {
            for (unsigned arm = 0; arm < BENCH_CHANNELS; arm++)
                d_fft.get_inbuf()[BENCH_CHANNELS - arm - 1] =
                    d_fir_filters[BENCH_CHANNELS - arm - 1].filter(&arms[arm][frame]);
            d_fft.execute();
            std::memcpy(out + frame * BENCH_CHANNELS,
                        d_fft.get_outbuf(),
                        sizeof(complex) * BENCH_CHANNELS);
        }
    }
};

struct big_fft {
    samples input;
    gr::fft::fft_complex_fwd fft;

    explicit big_fft(unsigned n) : input(n), fft(int(n), 1)
    {
        bench_signal(reinterpret_cast<float*>(input.data()), n, 0xFF7);
    }
};

struct context {
    unsigned n;
    unsigned long_n;
    samples x;
    samples padded;
    samples y;
    std::vector<float> real;
    std::unique_ptr<gr::filter::kernel::fir_filter_ccf> fir;
    std::unique_ptr<gr::filter::kernel::fft_filter_ccf> long_fir;
    std::unique_ptr<channel_bank> channels;
    std::unique_ptr<arb_stage> resample;
    gr::blocks::rotator rotator;
    std::unique_ptr<xlating_stage> xlate;
    std::unique_ptr<arb_stage> ddc_resample;
    std::unique_ptr<gr::fft::fft_complex_fwd> fft;
    gr::analog::quadrature_demod_cf::sptr fm;
};

void fir_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    c->fir->filterN(c->y.data(), c->padded.data(), c->n);
}

void long_fir_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    c->long_fir->filter(int(c->long_n), c->x.data(), c->y.data());
}

void channelize_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    c->channels->process(c->x.data(), c->n, c->y.data());
}

void big_fft_step(void* raw)
{
    auto* f = static_cast<big_fft*>(raw);
    std::memcpy(f->fft.get_inbuf(), f->input.data(), sizeof(complex) * f->input.size());
    f->fft.execute();
}

void measure_fft(const char* id, unsigned n, const bench_timing* timing)
{
    big_fft f(n);
    bench_run(id, big_fft_step, &f, n, timing);
}

void decimate_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    c->fir->filterNdec(c->y.data(), c->padded.data(), c->n / BENCH_DECIMATION, BENCH_DECIMATION);
}

void resample_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    c->resample->process(c->x.data(), c->n);
}

void nco_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    c->rotator.rotateN(c->y.data(), c->x.data(), int(c->n));
}

void ddc_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    c->xlate->process(c->x.data(), c->n);
    c->ddc_resample->process(c->xlate->output.data(), c->xlate->output.size());
}

void fft_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    std::memcpy(c->fft->get_inbuf(), c->x.data(), sizeof(complex) * BENCH_FFT);
    c->fft->execute();
}

void fm_step(void* raw)
{
    auto* c = static_cast<context*>(raw);
    gr_vector_const_void_star in{ c->padded.data() };
    gr_vector_void_star out{ c->real.data() };
    c->fm->work(int(c->n), in, out);
}

std::vector<float> fir_taps()
{
    std::vector<float> taps(BENCH_TAPS);
    bench_taps(taps.data(), BENCH_TAPS, BENCH_CUTOFF);
    return taps;
}

unsigned fft_filter_block(unsigned ntaps)
{
    unsigned size = 2;
    while (size < 2 * ntaps)
        size *= 2;
    return size - ntaps + 1;
}

std::vector<float> long_taps()
{
    std::vector<float> taps(BENCH_LONG_TAPS);
    bench_taps(taps.data(), BENCH_LONG_TAPS, BENCH_CUTOFF);
    return taps;
}

std::vector<float> channel_taps()
{
    std::vector<float> taps(BENCH_CHANNELS * BENCH_CHANNEL_TAPS);
    bench_taps(taps.data(), unsigned(taps.size()), 0.5 / BENCH_CHANNELS);
    return taps;
}

std::vector<float> arb_taps(double rate)
{
    double cutoff = 0.4 * std::min(1.0, rate);
    return gr::filter::firdes::low_pass_2(
        arb_filters, arb_filters, cutoff, 0.2 * std::min(1.0, rate), 100.0);
}

std::vector<float> xlating_taps()
{
    double stage_rate = ddc_input_rate / ddc_decimation;
    return gr::filter::firdes::low_pass_2(
        1.0, ddc_input_rate, ddc_passband, stage_rate - ddc_protect - ddc_passband, stopband_db);
}

std::vector<float> ddc_arb_taps()
{
    double stage_rate = ddc_input_rate / ddc_decimation;
    return gr::filter::firdes::low_pass_2(arb_filters,
                                          arb_filters * stage_rate,
                                          ddc_passband,
                                          ddc_output_rate - ddc_protect - ddc_passband,
                                          stopband_db);
}

void setup(context& c, unsigned n)
{
    c.n = n;
    c.x.resize(n);
    bench_signal(reinterpret_cast<float*>(c.x.data()), n, 0x11D);
    c.padded = samples(BENCH_TAPS - 1, complex{});
    c.padded.insert(c.padded.end(), c.x.begin(), c.x.end());
    c.y.resize(2 * n);
    c.real.resize(n);
    c.fir = std::make_unique<gr::filter::kernel::fir_filter_ccf>(fir_taps());
    unsigned block = fft_filter_block(BENCH_LONG_TAPS);
    c.long_n = n - n % block;
    c.long_fir = std::make_unique<gr::filter::kernel::fft_filter_ccf>(1, long_taps());
    c.channels = std::make_unique<channel_bank>(channel_taps());
    double resample_rate = 48000.0 / 44100.0;
    c.resample = std::make_unique<arb_stage>(float(resample_rate), arb_taps(resample_rate));
    c.rotator.set_phase_incr(std::polar(1.0f, float(2.0 * M_PI * ddc_offset / ddc_input_rate)));
    c.xlate = std::make_unique<xlating_stage>(xlating_taps());
    c.ddc_resample = std::make_unique<arb_stage>(
        float(ddc_output_rate * ddc_decimation / ddc_input_rate), ddc_arb_taps());
    c.fft = std::make_unique<gr::fft::fft_complex_fwd>(BENCH_FFT, 1);
    c.fm = gr::analog::quadrature_demod_cf::make(0.5f);
}

}

void log_to_stderr()
{
    auto backend = std::static_pointer_cast<spdlog::sinks::dist_sink_mt>(
        gr::logging::singleton().default_backend());
    backend->set_sinks({ std::make_shared<spdlog::sinks::stderr_sink_mt>() });
}

int main(int argc, char** argv)
{
    log_to_stderr();
    bench_timing timing;
    if (bench_parse(argc, argv, &timing)) return 1;
    if (timing.block < BENCH_FFT) {
        std::fprintf(stderr, "block must hold one FFT\n");
        return 1;
    }
    context c;
    setup(c, timing.block);
    std::printf("version\t%s (VOLK %d.%d.%d)\n",
                gr::version().c_str(),
                VOLK_VERSION_MAJOR,
                VOLK_VERSION_MINOR,
                VOLK_VERSION_MAINT);
    bench_run("fir", fir_step, &c, c.n, &timing);
    bench_run("fir-long", long_fir_step, &c, c.long_n, &timing);
    bench_run("decimate", decimate_step, &c, c.n, &timing);
    bench_run("resample", resample_step, &c, c.n, &timing);
    bench_ratio("resample", resample_step, &c, c.n, &c.resample->produced);
    bench_run("nco", nco_step, &c, c.n, &timing);
    bench_run("ddc", ddc_step, &c, c.n, &timing);
    bench_ratio("ddc", ddc_step, &c, c.n, &c.ddc_resample->produced);
    bench_run("channelize", channelize_step, &c, c.n - c.n % BENCH_CHANNELS, &timing);
    bench_run("fft", fft_step, &c, BENCH_FFT, &timing);
    measure_fft("fft-64k", BENCH_BIG_FFT, &timing);
    measure_fft("fft-1m", BENCH_HUGE_FFT, &timing);
    bench_run("fm", fm_step, &c, c.n, &timing);
    return 0;
}

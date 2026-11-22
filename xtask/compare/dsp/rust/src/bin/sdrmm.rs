use num_complex::Complex32;
use sdrmm_compare_dsp::{
    BIG_FFT, CHANNELS, CUTOFF, DDC_INPUT_RATE, DDC_OFFSET, DDC_OUTPUT_RATE, DECIMATION, FFT,
    HUGE_FFT, LONG_TAPS, RESAMPLE_RATIO, TAPS, Timing, keep, ratio, run, signal, taps,
};
use sdrmm_dsp::{
    Channelizer, Ddc, Decimator, FastFir, FmDemod, FracResampler, Nco, math::fft::FftPair,
};

fn filters(timing: &Timing, input: &[Complex32]) {
    let taps = taps(TAPS, CUTOFF);
    let mut out = Vec::new();
    let mut fir = Decimator::new(&taps, 1);
    run("fir", timing.block, timing, || {
        fir.process(input, &mut out);
        keep(&out);
    });
    let mut long = FastFir::real(&sdrmm_compare_dsp::taps(LONG_TAPS, CUTOFF));
    run("fir-long", timing.block, timing, || {
        long.process(input, &mut out);
        keep(&out);
    });
    let mut decimator = Decimator::new(&taps, DECIMATION);
    run("decimate", timing.block, timing, || {
        decimator.process(input, &mut out);
        keep(&out);
    });
    let mut resampler = FracResampler::new(RESAMPLE_RATIO);
    run("resample", timing.block, timing, || {
        resampler.process(input, &mut out);
        keep(&out);
    });
    ratio("resample", timing.block, || {
        resampler.process(input, &mut out);
        out.len()
    });
}

fn tuning(timing: &Timing, input: &[Complex32]) -> Result<(), String> {
    let mut mixed = vec![Complex32::default(); input.len()];
    let mut nco = Nco::new(DDC_OFFSET as f32, DDC_INPUT_RATE as f32);
    run("nco", timing.block, timing, || {
        nco.mix_into(input, &mut mixed);
        keep(&mixed);
    });
    let mut ddc =
        Ddc::new(DDC_INPUT_RATE, DDC_OUTPUT_RATE, DDC_OFFSET).map_err(|err| err.to_string())?;
    let mut out = Vec::new();
    run("ddc", timing.block, timing, || {
        ddc.process(input, &mut out);
        keep(&out);
    });
    ratio("ddc", timing.block, || {
        ddc.process(input, &mut out);
        out.len()
    });
    channelize(timing, input)
}

fn channelize(timing: &Timing, input: &[Complex32]) -> Result<(), String> {
    let half = CHANNELS as i32 / 2;
    let bins: Vec<i32> = (-half..half).collect();
    let cutoff = 0.5 / CHANNELS as f64;
    let mut bank =
        Channelizer::new(CHANNELS, CHANNELS, cutoff, &bins).map_err(|err| err.to_string())?;
    let mut lanes = vec![Vec::new(); CHANNELS];
    let usable = &input[..input.len() - input.len() % CHANNELS];
    run("channelize", usable.len(), timing, || {
        bank.process(usable, &mut lanes);
        keep(&lanes);
    });
    Ok(())
}

fn transform(id: &str, size: usize, timing: &Timing) {
    let input = signal(size, 0xFF7);
    let mut fft = FftPair::new(size);
    let mut work = vec![Complex32::default(); size];
    run(id, size, timing, || {
        work.copy_from_slice(&input);
        fft.forward(&mut work);
        keep(&work);
    });
}

fn spectra(timing: &Timing, input: &[Complex32]) {
    transform("fft", FFT, timing);
    transform("fft-64k", BIG_FFT, timing);
    transform("fft-1m", HUGE_FFT, timing);
    let mut fm = FmDemod::new(240e3, 75e3);
    let mut audio = Vec::new();
    run("fm", timing.block, timing, || {
        fm.process(input, &mut audio);
        keep(&audio);
    });
}

fn main() -> Result<(), String> {
    let timing = Timing::from_env()?;
    let input = signal(timing.block, 0x11D);
    filters(&timing, &input);
    tuning(&timing, &input)?;
    spectra(&timing, &input);
    Ok(())
}

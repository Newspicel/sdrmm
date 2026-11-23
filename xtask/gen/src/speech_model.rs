//! Converts NVIDIA's Parakeet TDT checkpoint into the tensor layout `sdrmm_dsp::audio::speech`
//! reads, so the app downloads one file and needs no Python.

mod nemo;

use std::{
    collections::HashMap,
    fs,
    path::{Path, PathBuf},
    process::Command,
};

use anyhow::{Context, Result, bail, ensure};
use clap::Args;
use sdrmm_dsp::math::neural::{Graph, Value, Weights, f32_to_f16};
use sdrmm_wire::{SPEECH_MODELS_PREFIX, SpeechModel};
use serde_json::Value as Json;
use sha2::{Digest, Sha256};

const BASE: &str = "https://huggingface.co/nvidia/parakeet-tdt-0.6b-v3/resolve/541d1f99c6b0c3cd0b11a95167540bb8edefd82b";
const BASE_FILES: [(&str, &str); 4] = [
    (
        "model.safetensors",
        "3a2026366188c8c68598edbbff92f8d11590a08e0ae2e6775544e7b07d6a5e11",
    ),
    ("config.json", ""),
    ("processor_config.json", ""),
    ("tokenizer.json", ""),
];
const ATC: &str = "https://huggingface.co/qenneth/parakeet-tdt-0.6b-v3-finetuned-for-ATC/resolve/ac5200eec345e4e674a8a685ef52ab6b2cf4fa95/parakeet-tdt-0.6b-v3-finetuned-for-ATC.nemo";
const ATC_FILE: &str = "parakeet-tdt-0.6b-v3-atc.nemo";
const ATC_SHA256: &str = "423a9cda1fe0d2532a08d75c62b4787adfaec636439fabc3c28873cbb437ff2a";
const IGNORED_SUFFIX: &str = ".num_batches_tracked";
const BATCH_NORM_EPS: f32 = 1e-5;

/// Where a model's weights come from and under what licence, kept in the converted file.
struct Origin {
    source: &'static str,
    license: &'static str,
}

const fn origin(model: SpeechModel) -> Origin {
    match model {
        SpeechModel::ParakeetTdtV3 => Origin {
            source: "nvidia/parakeet-tdt-0.6b-v3",
            license: "CC-BY-4.0",
        },
        SpeechModel::ParakeetTdtV3Atc => Origin {
            source: "qenneth/parakeet-tdt-0.6b-v3-finetuned-for-ATC",
            license: "MIT, from nvidia/parakeet-tdt-0.6b-v3 under CC-BY-4.0",
        },
    }
}

#[derive(Args)]
pub(crate) struct SpeechModelArgs {
    /// A Hugging Face style directory to convert instead of the pinned releases.
    #[arg(long)]
    from: Option<PathBuf>,
    /// Where to write a model converted with --from.
    #[arg(long)]
    out: Option<PathBuf>,
}

pub(crate) fn run(root: &Path, args: &SpeechModelArgs) -> Result<()> {
    if let Some(dir) = &args.from {
        let checkpoint = Checkpoint::safetensors(&dir.join("model.safetensors"))?;
        let out = args.out.clone().context("--from needs --out")?;
        write(
            &out,
            &convert(dir, checkpoint, &origin(SpeechModel::ParakeetTdtV3))?.encode(),
        )?;
        return Ok(());
    }
    let scratch = root.join("target/speech-model");
    fs::create_dir_all(&scratch)?;
    for (name, sha256) in BASE_FILES {
        download(&format!("{BASE}/{name}"), sha256, &scratch.join(name))?;
    }
    let mut stale = Vec::new();
    for model in SpeechModel::ALL {
        let checkpoint = match model {
            SpeechModel::ParakeetTdtV3 => {
                Checkpoint::safetensors(&scratch.join("model.safetensors"))?
            }
            SpeechModel::ParakeetTdtV3Atc => {
                let archive = scratch.join(ATC_FILE);
                download(ATC, ATC_SHA256, &archive)?;
                Checkpoint::nemo(&archive)?
            }
        };
        let bytes = convert(&scratch, checkpoint, &origin(model))?.encode();
        let digest = write(&scratch.join(model.file_name()), &bytes)?;
        let listed = model.artifact();
        if listed.sha256 != digest || listed.bytes != bytes.len() as u64 {
            stale.push(format!(
                "{}: {} bytes, sha256 {digest}",
                model.name(),
                bytes.len()
            ));
        }
    }
    ensure!(
        stale.is_empty(),
        "update the catalog in crates/wire/src/speech.rs:\n{}",
        stale.join("\n")
    );
    println!(
        "upload with: scripts/r2-upload.sh {SPEECH_MODELS_PREFIX} {}/*.sdrmmnn",
        scratch.display()
    );
    Ok(())
}

fn write(path: &Path, bytes: &[u8]) -> Result<String> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    fs::write(path, bytes).with_context(|| format!("write {}", path.display()))?;
    let digest = hex(&Sha256::digest(bytes));
    println!("{}: {} bytes, sha256 {digest}", path.display(), bytes.len());
    Ok(digest)
}

fn download(url: &str, sha256: &str, to: &Path) -> Result<()> {
    if !to.exists() {
        println!("fetching {url}");
        let status = Command::new("curl")
            .args(["-fsSL", "--retry", "3", "-o"])
            .arg(to)
            .arg(url)
            .status()
            .context("curl not found")?;
        ensure!(status.success(), "curl failed for {url}");
    }
    if sha256.is_empty() {
        return Ok(());
    }
    let digest = hex(&Sha256::digest(fs::read(to)?));
    if digest != sha256 {
        fs::remove_file(to)?;
        bail!("{url} changed upstream: sha256 {digest}");
    }
    Ok(())
}

/// A `NeMo` tensor name as the Hugging Face port of the same model spells it.
fn hugging_face_name(nemo: &str) -> String {
    const RENAMES: [(&str, &str); 14] = [
        ("encoder.pre_encode.conv.", "encoder.subsampling.layers."),
        ("encoder.pre_encode.out.", "encoder.subsampling.linear."),
        (".conv.batch_norm.", ".conv.norm."),
        (".self_attn.pos_bias_u", ".self_attn.bias_u"),
        (".self_attn.pos_bias_v", ".self_attn.bias_v"),
        (".self_attn.linear_q.", ".self_attn.q_proj."),
        (".self_attn.linear_k.", ".self_attn.k_proj."),
        (".self_attn.linear_v.", ".self_attn.v_proj."),
        (".self_attn.linear_out.", ".self_attn.o_proj."),
        (".self_attn.linear_pos.", ".self_attn.relative_k_proj."),
        ("decoder.prediction.embed.", "decoder.embedding."),
        ("decoder.prediction.dec_rnn.lstm.", "decoder.lstm."),
        ("joint.pred.", "decoder.decoder_projector."),
        ("joint.enc.", "encoder_projector."),
    ];
    let renamed = RENAMES
        .iter()
        .fold(nemo.to_owned(), |name, (from, to)| name.replace(from, to));
    renamed.replace("joint.joint_net.2.", "joint.head.")
}

fn hex(bytes: &[u8]) -> String {
    bytes.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn json(path: &Path) -> Result<Json> {
    let text = fs::read_to_string(path).with_context(|| format!("read {}", path.display()))?;
    serde_json::from_str(&text).with_context(|| format!("parse {}", path.display()))
}

fn number(config: &Json, path: &[&str]) -> Result<usize> {
    path.iter()
        .try_fold(config, |at, key| at.get(key))
        .and_then(Json::as_u64)
        .map(|value| value as usize)
        .with_context(|| format!("config lacks {}", path.join(".")))
}

/// The checkpoint's tensors, checked off as the layout takes them.
struct Checkpoint {
    entries: HashMap<String, nemo::Tensor>,
}

impl Checkpoint {
    fn safetensors(path: &Path) -> Result<Self> {
        let bytes = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let head = bytes.get(..8).context("safetensors header")?;
        let len = u64::from_le_bytes(head.try_into()?) as usize;
        let header: HashMap<String, Json> =
            serde_json::from_slice(bytes.get(8..8 + len).context("safetensors header")?)?;
        let base = 8 + len;
        let mut entries = HashMap::new();
        for (name, entry) in header {
            if name == "__metadata__" || name.ends_with(IGNORED_SUFFIX) {
                continue;
            }
            ensure!(entry["dtype"] == "F32", "{name} is not F32");
            let shape = entry["shape"]
                .as_array()
                .context("shape")?
                .iter()
                .map(|dim| dim.as_u64().map(|d| d as usize).context("shape"))
                .collect::<Result<_>>()?;
            let offsets = entry["data_offsets"].as_array().context("offsets")?;
            let start = offsets.first().and_then(Json::as_u64).context("offsets")? as usize;
            let end = offsets.get(1).and_then(Json::as_u64).context("offsets")? as usize;
            let raw = bytes
                .get(base + start..base + end)
                .with_context(|| format!("{name} runs past the file"))?;
            let values = raw
                .as_chunks::<4>()
                .0
                .iter()
                .map(|quad| f32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
                .collect();
            entries.insert(name, (shape, values));
        }
        Ok(Self { entries })
    }

    /// A `NeMo` archive with its tensors renamed to the Hugging Face layout the converter reads.
    fn nemo(path: &Path) -> Result<Self> {
        let archive = fs::read(path).with_context(|| format!("read {}", path.display()))?;
        let entries = nemo::tensors(&archive)?
            .into_iter()
            .filter(|(name, _)| !name.starts_with("preprocessor."))
            .map(|(name, tensor)| (hugging_face_name(&name), tensor))
            .collect();
        Ok(Self { entries })
    }

    fn take(&mut self, name: &str) -> Result<nemo::Tensor> {
        let tensor = self
            .entries
            .remove(name)
            .with_context(|| format!("checkpoint lacks {name}"))?;
        Ok(tensor)
    }

    fn leftover(&self) -> Vec<&String> {
        self.entries.keys().collect()
    }
}

/// The converted model under construction: tensors in order, named in the metadata.
struct Layout {
    names: Vec<String>,
    values: Vec<Value>,
}

impl Layout {
    fn push(&mut self, name: String, shape: Vec<usize>, data: Weights) {
        self.names.push(name);
        self.values.push(Value {
            shape,
            data: Some(data),
        });
    }

    fn floats(&mut self, name: String, data: Vec<f32>) {
        let len = data.len();
        self.push(name, vec![len], Weights::F32(data));
    }

    fn matrix(&mut self, name: String, rows: usize, data: &[f32]) {
        let cols = data.len() / rows;
        self.push(
            name,
            vec![rows, cols],
            Weights::F16(data.iter().map(|&x| f32_to_f16(x)).collect()),
        );
    }

    fn full_matrix(&mut self, name: String, rows: usize, data: Vec<f32>) {
        let cols = data.len() / rows;
        self.push(name, vec![rows, cols], Weights::F32(data));
    }
}

/// `[out, in, ...]` weights as `[in, out]`.
fn transposed(shape: &[usize], data: &[f32]) -> (usize, Vec<f32>) {
    let (rows, cols) = (shape[0], data.len() / shape[0]);
    let mut out = vec![0.0; data.len()];
    for r in 0..rows {
        for c in 0..cols {
            out[c * rows + r] = data[r * cols + c];
        }
    }
    (cols, out)
}

fn convert(dir: &Path, mut checkpoint: Checkpoint, origin: &Origin) -> Result<Graph> {
    let config = json(&dir.join("config.json"))?;
    let processor = json(&dir.join("processor_config.json"))?;
    let encoder = &config["encoder_config"];
    ensure!(
        encoder["scale_input"] == false,
        "scaled input is not supported"
    );
    ensure!(
        encoder["hidden_act"] == "silu",
        "only SiLU encoders are supported"
    );
    ensure!(
        number(encoder, &["subsampling_conv_kernel_size"])? == 3
            && number(encoder, &["subsampling_conv_stride"])? == 2,
        "only 3x3 stride-2 subsampling is supported"
    );
    let layers = number(encoder, &["num_hidden_layers"])?;
    let stages = number(encoder, &["subsampling_factor"])?.trailing_zeros() as usize - 1;
    let decoder_layers = number(&config, &["num_decoder_layers"])?;
    let mut layout = Layout {
        names: Vec::new(),
        values: Vec::new(),
    };
    subsampling(&mut checkpoint, &mut layout, stages)?;
    for layer in 0..layers {
        block(&mut checkpoint, &mut layout, layer)?;
    }
    transducer(&mut checkpoint, &mut layout, decoder_layers)?;
    let leftover = checkpoint.leftover();
    ensure!(leftover.is_empty(), "unconverted tensors: {leftover:?}");
    let features = &processor["feature_extractor"];
    let durations = config["durations"]
        .as_array()
        .context("durations")?
        .iter()
        .map(std::string::ToString::to_string)
        .collect::<Vec<_>>()
        .join(",");
    let meta = vec![
        ("model", "parakeet-tdt".to_owned()),
        ("source", origin.source.to_owned()),
        ("license", origin.license.to_owned()),
        (
            "mel_bands",
            number(features, &["feature_size"])?.to_string(),
        ),
        ("n_fft", number(features, &["n_fft"])?.to_string()),
        ("win_length", number(features, &["win_length"])?.to_string()),
        ("hop_length", number(features, &["hop_length"])?.to_string()),
        (
            "preemphasis",
            features["preemphasis"]
                .as_f64()
                .context("preemphasis")?
                .to_string(),
        ),
        ("layers", layers.to_string()),
        (
            "heads",
            number(encoder, &["num_attention_heads"])?.to_string(),
        ),
        ("subsampling_stages", stages.to_string()),
        ("decoder_layers", decoder_layers.to_string()),
        ("blank", number(&config, &["blank_token_id"])?.to_string()),
        ("durations", durations),
        ("vocab", vocab(&dir.join("tokenizer.json"))?),
        ("tensors", layout.names.join("\n")),
    ];
    Ok(Graph {
        meta: meta.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
        values: layout.values,
        nodes: Vec::new(),
        inputs: Vec::new(),
        outputs: Vec::new(),
    })
}

fn vocab(path: &Path) -> Result<String> {
    let tokenizer = json(path)?;
    let pieces = tokenizer["model"]["vocab"].as_object().context("vocab")?;
    let mut by_id = vec![String::new(); pieces.len()];
    for (piece, id) in pieces {
        let id = id.as_u64().context("vocab id")? as usize;
        by_id
            .get_mut(id)
            .context("vocab id out of range")?
            .clone_from(piece);
    }
    for added in tokenizer["added_tokens"]
        .as_array()
        .context("added tokens")?
    {
        let content = added["content"].as_str().unwrap_or_default();
        let control =
            added["special"] == true || (content.starts_with("<|") && content.ends_with("|>"));
        if let Some(slot) = added["id"]
            .as_u64()
            .and_then(|id| by_id.get_mut(id as usize))
            .filter(|_| control)
        {
            slot.clear();
        }
    }
    ensure!(
        by_id.iter().all(|piece| !piece.contains('\n')),
        "a piece holds a newline"
    );
    Ok(by_id.join("\n"))
}

fn subsampling(checkpoint: &mut Checkpoint, layout: &mut Layout, stages: usize) -> Result<()> {
    let at = |layer: usize, part: &str| format!("encoder.subsampling.layers.{layer}.{part}");
    let (_, weight) = checkpoint.take(&at(0, "weight"))?;
    layout.floats("subsampling.first.weight".into(), weight);
    layout.floats(
        "subsampling.first.bias".into(),
        checkpoint.take(&at(0, "bias"))?.1,
    );
    for stage in 0..stages {
        let (depthwise, pointwise) = (2 + 3 * stage, 3 + 3 * stage);
        let name = |part: &str| format!("subsampling.{stage}.{part}");
        layout.floats(
            name("depthwise.weight"),
            checkpoint.take(&at(depthwise, "weight"))?.1,
        );
        layout.floats(
            name("depthwise.bias"),
            checkpoint.take(&at(depthwise, "bias"))?.1,
        );
        let (shape, weight) = checkpoint.take(&at(pointwise, "weight"))?;
        let (rows, data) = transposed(&shape, &weight);
        layout.matrix(name("pointwise.weight"), rows, &data);
        layout.floats(
            name("pointwise.bias"),
            checkpoint.take(&at(pointwise, "bias"))?.1,
        );
    }
    let (shape, weight) = checkpoint.take("encoder.subsampling.linear.weight")?;
    let (rows, data) = transposed(&shape, &weight);
    layout.matrix("subsampling.linear.weight".into(), rows, &data);
    layout.floats(
        "subsampling.linear.bias".into(),
        checkpoint.take("encoder.subsampling.linear.bias")?.1,
    );
    Ok(())
}

fn linear(checkpoint: &mut Checkpoint, layout: &mut Layout, from: &str, to: String) -> Result<()> {
    let (shape, weight) = checkpoint.take(from)?;
    let (rows, data) = transposed(&shape, &weight);
    layout.matrix(to, rows, &data);
    Ok(())
}

fn block(checkpoint: &mut Checkpoint, layout: &mut Layout, layer: usize) -> Result<()> {
    let from = |part: &str| format!("encoder.layers.{layer}.{part}");
    let to = |part: &str| format!("layers.{layer}.{part}");
    for (source, target) in [
        ("feed_forward1.linear1.weight", "feed_forward1.up"),
        ("feed_forward1.linear2.weight", "feed_forward1.down"),
        ("self_attn.q_proj.weight", "attention.query"),
        ("self_attn.k_proj.weight", "attention.key"),
        ("self_attn.v_proj.weight", "attention.value"),
        ("self_attn.o_proj.weight", "attention.output"),
        ("self_attn.relative_k_proj.weight", "attention.position"),
        ("conv.pointwise_conv1.weight", "conv.expand"),
        ("conv.pointwise_conv2.weight", "conv.project"),
        ("feed_forward2.linear1.weight", "feed_forward2.up"),
        ("feed_forward2.linear2.weight", "feed_forward2.down"),
    ] {
        linear(checkpoint, layout, &from(source), to(target))?;
    }
    for bias in ["bias_u", "bias_v"] {
        let (_, data) = checkpoint.take(&from(&format!("self_attn.{bias}")))?;
        layout.floats(to(&format!("attention.{bias}")), data);
    }
    depthwise(checkpoint, layout, &from("conv"), &to("conv.depthwise"))?;
    for (source, target) in [
        ("norm_feed_forward1", "norm_feed_forward1"),
        ("norm_self_att", "norm_attention"),
        ("norm_conv", "norm_conv"),
        ("norm_feed_forward2", "norm_feed_forward2"),
        ("norm_out", "norm_out"),
    ] {
        for part in ["weight", "bias"] {
            let (_, data) = checkpoint.take(&from(&format!("{source}.{part}")))?;
            layout.floats(to(&format!("{target}.{part}")), data);
        }
    }
    Ok(())
}

/// The depthwise convolution with the batch norm after it folded in.
fn depthwise(checkpoint: &mut Checkpoint, layout: &mut Layout, from: &str, to: &str) -> Result<()> {
    let (shape, mut weight) = checkpoint.take(&format!("{from}.depthwise_conv.weight"))?;
    let channels = shape[0];
    let kernel = weight.len() / channels;
    let (_, gamma) = checkpoint.take(&format!("{from}.norm.weight"))?;
    let (_, beta) = checkpoint.take(&format!("{from}.norm.bias"))?;
    let (_, mean) = checkpoint.take(&format!("{from}.norm.running_mean"))?;
    let (_, var) = checkpoint.take(&format!("{from}.norm.running_var"))?;
    let mut bias = vec![0.0; channels];
    for c in 0..channels {
        let scale = gamma[c] / (var[c] + BATCH_NORM_EPS).sqrt();
        for w in &mut weight[c * kernel..(c + 1) * kernel] {
            *w *= scale;
        }
        bias[c] = beta[c] - mean[c] * scale;
    }
    layout.floats(format!("{to}.weight"), weight);
    layout.floats(format!("{to}.bias"), bias);
    Ok(())
}

fn transducer(checkpoint: &mut Checkpoint, layout: &mut Layout, layers: usize) -> Result<()> {
    linear(
        checkpoint,
        layout,
        "encoder_projector.weight",
        "encoder_projector.weight".into(),
    )?;
    layout.floats(
        "encoder_projector.bias".into(),
        checkpoint.take("encoder_projector.bias")?.1,
    );
    let (shape, embedding) = checkpoint.take("decoder.embedding.weight")?;
    layout.matrix("decoder.embedding".into(), shape[0], &embedding);
    for layer in 0..layers {
        let part = |name: &str| format!("decoder.lstm.{name}_l{layer}");
        let (input_shape, input) = checkpoint.take(&part("weight_ih"))?;
        let (hidden_shape, hidden) = checkpoint.take(&part("weight_hh"))?;
        let (rows, mut joined) = transposed(&input_shape, &input);
        let (hidden_rows, hidden_t) = transposed(&hidden_shape, &hidden);
        joined.extend(hidden_t);
        layout.full_matrix(
            format!("decoder.lstm.{layer}.weight"),
            rows + hidden_rows,
            joined,
        );
        let (_, mut bias) = checkpoint.take(&part("bias_ih"))?;
        let (_, other) = checkpoint.take(&part("bias_hh"))?;
        for (b, o) in bias.iter_mut().zip(other) {
            *b += o;
        }
        layout.floats(format!("decoder.lstm.{layer}.bias"), bias);
    }
    for (from, to) in [
        ("decoder.decoder_projector", "decoder.projector"),
        ("joint.head", "joint"),
    ] {
        let (shape, weight) = checkpoint.take(&format!("{from}.weight"))?;
        let (rows, data) = transposed(&shape, &weight);
        layout.full_matrix(format!("{to}.weight"), rows, data);
        layout.floats(
            format!("{to}.bias"),
            checkpoint.take(&format!("{from}.bias"))?.1,
        );
    }
    Ok(())
}

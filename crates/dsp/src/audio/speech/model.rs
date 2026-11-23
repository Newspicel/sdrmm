use std::collections::HashMap;

use super::{
    SpeechError,
    attention::{Attention, relative_positions},
    conformer::{Block, ConvModule, FeedForward, Pass},
    matmul::{HalfMatrix, matmul},
    mel::LogMel,
    subsample::{Conv3, Stage, Subsampling},
    tensor::{Norm, add_rows},
    transducer::{Lstm, Transducer},
    vocab::Vocab,
};
use crate::math::{
    neural::{Graph, Index, Stored, f16_to_f32},
    vector::Vector,
};

/// The rate a model hears; callers resample clips to it.
pub const SAMPLE_RATE_HZ: u32 = 16_000;
const KIND: &str = "parakeet-tdt";
/// The longest stretch run through the encoder at once, which keeps attention memory bounded.
const MAX_CHUNK_SAMPLES: usize = 30 * SAMPLE_RATE_HZ as usize;

/// A Parakeet TDT speech recogniser, its large matrices read in place from the model bytes.
pub struct Parakeet {
    bytes: Vec<u8>,
    vector: Vector,
    bands: usize,
    stft: (usize, usize, usize),
    preemphasis: f32,
    subsampling: Subsampling,
    blocks: Vec<Block>,
    projector: HalfMatrix,
    projector_bias: Vec<f32>,
    transducer: Transducer,
    vocab: Vocab,
}

struct Tensors<'a> {
    bytes: &'a [u8],
    index: &'a Index,
    named: HashMap<&'a str, &'a Stored>,
}

impl Parakeet {
    pub fn load(bytes: Vec<u8>) -> Result<Self, SpeechError> {
        let index = Graph::index(&bytes)?;
        if index.meta("model") != Some(KIND) {
            return Err(SpeechError::Model("not a Parakeet TDT model".into()));
        }
        let tensors = Tensors::new(&bytes, &index)?;
        let model = Self {
            vector: Vector::detect(),
            bands: tensors.number("mel_bands")?,
            stft: (
                tensors.number("n_fft")?,
                tensors.number("win_length")?,
                tensors.number("hop_length")?,
            ),
            preemphasis: tensors.number("preemphasis")?,
            subsampling: tensors.subsampling()?,
            blocks: (0..tensors.number::<usize>("layers")?)
                .map(|layer| tensors.block(layer))
                .collect::<Result<_, _>>()?,
            projector: tensors.half("encoder_projector.weight")?,
            projector_bias: tensors.floats("encoder_projector.bias")?,
            transducer: tensors.transducer()?,
            vocab: Vocab::parse(tensors.text("vocab")?),
            bytes: Vec::new(),
        };
        if model.vocab.len() != model.transducer.blank {
            return Err(SpeechError::Model(
                "vocabulary does not match the joint".into(),
            ));
        }
        Ok(Self { bytes, ..model })
    }

    /// The text spoken in `pcm`, mono at [`SAMPLE_RATE_HZ`], computed on up to `threads` threads.
    pub fn transcribe(&self, pcm: &[f32], threads: usize) -> Result<String, SpeechError> {
        let chunks = pcm.len().div_ceil(MAX_CHUNK_SAMPLES).max(1);
        let len = pcm.len().div_ceil(chunks).max(1);
        let mut words = Vec::new();
        for chunk in pcm.chunks(len) {
            let text = self.transcribe_chunk(chunk, threads)?;
            if !text.is_empty() {
                words.push(text);
            }
        }
        Ok(words.join(" "))
    }

    fn transcribe_chunk(&self, pcm: &[f32], threads: usize) -> Result<String, SpeechError> {
        Ok(self.vocab.decode(&self.tokens(pcm, threads)?))
    }

    pub(super) fn tokens(&self, pcm: &[f32], threads: usize) -> Result<Vec<usize>, SpeechError> {
        if pcm.len() < 2 * self.stft.2 {
            return Ok(Vec::new());
        }
        let (encoded, time) = self.encode(pcm, threads)?;
        let width = self.projector.cols;
        let mut projected = vec![0.0; time * width];
        matmul(
            &self.bytes,
            &self.projector,
            &encoded,
            time,
            &mut projected,
            threads,
        );
        add_rows(&mut projected, &self.projector_bias);
        Ok(self
            .transducer
            .decode(&self.bytes, self.vector, &projected, time))
    }

    /// The encoder's output frames and how many there are.
    pub(super) fn encode(
        &self,
        pcm: &[f32],
        threads: usize,
    ) -> Result<(Vec<f32>, usize), SpeechError> {
        let mut mel = LogMel::new(SAMPLE_RATE_HZ, self.bands, self.stft, self.preemphasis);
        let features = mel.features(pcm)?;
        let (mut hidden, time) = self
            .subsampling
            .run(&self.bytes, &features, self.bands, threads);
        let positions = relative_positions(time, self.subsampling.linear.cols);
        let pass = Pass {
            bytes: &self.bytes,
            vector: self.vector,
            positions: &positions,
            time,
            threads,
        };
        for block in &self.blocks {
            block.run(&pass, &mut hidden);
        }
        Ok((hidden, time))
    }
}

impl<'a> Tensors<'a> {
    fn new(bytes: &'a [u8], index: &'a Index) -> Result<Self, SpeechError> {
        let names = index
            .meta("tensors")
            .ok_or_else(|| SpeechError::Model("no tensor names".into()))?;
        let named = names
            .split('\n')
            .zip(&index.tensors)
            .filter_map(|(name, stored)| Some((name, stored.as_ref()?)))
            .collect();
        Ok(Self {
            bytes,
            index,
            named,
        })
    }

    fn text(&self, key: &str) -> Result<&'a str, SpeechError> {
        self.index
            .meta(key)
            .ok_or_else(|| SpeechError::Model(format!("no {key}")))
    }

    fn number<T: std::str::FromStr>(&self, key: &str) -> Result<T, SpeechError> {
        self.text(key)?
            .trim()
            .parse()
            .map_err(|_| SpeechError::Model(format!("{key} is not a number")))
    }

    fn stored(&self, name: &str) -> Result<&'a Stored, SpeechError> {
        self.named
            .get(name)
            .copied()
            .ok_or_else(|| SpeechError::Model(format!("no tensor {name}")))
    }

    fn half(&self, name: &str) -> Result<HalfMatrix, SpeechError> {
        match self.stored(name)? {
            Stored {
                shape,
                half: true,
                at,
                ..
            } if shape.len() == 2 => Ok(HalfMatrix {
                at: *at,
                depth: shape[0],
                cols: shape[1],
            }),
            _ => Err(SpeechError::Model(format!("{name} is not a half matrix"))),
        }
    }

    fn floats(&self, name: &str) -> Result<Vec<f32>, SpeechError> {
        let stored = self.stored(name)?;
        let width = if stored.half { 2 } else { 4 };
        let raw = &self.bytes[stored.at..stored.at + stored.len * width];
        Ok(if stored.half {
            raw.as_chunks::<2>()
                .0
                .iter()
                .map(|pair| f16_to_f32(u16::from_le_bytes([pair[0], pair[1]])))
                .collect()
        } else {
            raw.as_chunks::<4>()
                .0
                .iter()
                .map(|quad| f32::from_le_bytes([quad[0], quad[1], quad[2], quad[3]]))
                .collect()
        })
    }

    fn norm(&self, name: &str) -> Result<Norm, SpeechError> {
        Ok(Norm {
            weight: self.floats(&format!("{name}.weight"))?,
            bias: self.floats(&format!("{name}.bias"))?,
        })
    }

    fn conv3(&self, name: &str) -> Result<Conv3, SpeechError> {
        Ok(Conv3 {
            weight: self.floats(&format!("{name}.weight"))?,
            bias: self.floats(&format!("{name}.bias"))?,
        })
    }

    fn subsampling(&self) -> Result<Subsampling, SpeechError> {
        let first = self.conv3("subsampling.first")?;
        let stages = (0..self.number::<usize>("subsampling_stages")?)
            .map(|stage| {
                Ok(Stage {
                    depthwise: self.conv3(&format!("subsampling.{stage}.depthwise"))?,
                    pointwise: self.half(&format!("subsampling.{stage}.pointwise.weight"))?,
                    pointwise_bias: self.floats(&format!("subsampling.{stage}.pointwise.bias"))?,
                })
            })
            .collect::<Result<_, SpeechError>>()?;
        Ok(Subsampling {
            channels: first.bias.len(),
            first,
            stages,
            linear: self.half("subsampling.linear.weight")?,
            linear_bias: self.floats("subsampling.linear.bias")?,
        })
    }

    fn block(&self, layer: usize) -> Result<Block, SpeechError> {
        let at = |name: &str| format!("layers.{layer}.{name}");
        let feed_forward = |name: &str| -> Result<FeedForward, SpeechError> {
            Ok(FeedForward {
                up: self.half(&at(&format!("{name}.up")))?,
                down: self.half(&at(&format!("{name}.down")))?,
            })
        };
        let depthwise = self.floats(&at("conv.depthwise.weight"))?;
        let depthwise_bias = self.floats(&at("conv.depthwise.bias"))?;
        Ok(Block {
            feed_forward1: feed_forward("feed_forward1")?,
            attention: Attention {
                query: self.half(&at("attention.query"))?,
                key: self.half(&at("attention.key"))?,
                value: self.half(&at("attention.value"))?,
                output: self.half(&at("attention.output"))?,
                position: self.half(&at("attention.position"))?,
                bias_u: self.floats(&at("attention.bias_u"))?,
                bias_v: self.floats(&at("attention.bias_v"))?,
                heads: self.number("heads")?,
            },
            conv: ConvModule {
                expand: self.half(&at("conv.expand"))?,
                kernel: depthwise.len() / depthwise_bias.len().max(1),
                depthwise,
                depthwise_bias,
                project: self.half(&at("conv.project"))?,
            },
            feed_forward2: feed_forward("feed_forward2")?,
            norm_feed_forward1: self.norm(&at("norm_feed_forward1"))?,
            norm_attention: self.norm(&at("norm_attention"))?,
            norm_conv: self.norm(&at("norm_conv"))?,
            norm_feed_forward2: self.norm(&at("norm_feed_forward2"))?,
            norm_out: self.norm(&at("norm_out"))?,
        })
    }

    fn transducer(&self) -> Result<Transducer, SpeechError> {
        let embedding = self.half("decoder.embedding")?;
        let durations = self
            .text("durations")?
            .split(',')
            .map(|value| {
                value
                    .trim()
                    .parse()
                    .map_err(|_| SpeechError::Model("durations are not numbers".into()))
            })
            .collect::<Result<_, _>>()?;
        Ok(Transducer {
            layers: (0..self.number::<usize>("decoder_layers")?)
                .map(|layer| {
                    Ok(Lstm {
                        weight: self.floats(&format!("decoder.lstm.{layer}.weight"))?,
                        bias: self.floats(&format!("decoder.lstm.{layer}.bias"))?,
                    })
                })
                .collect::<Result<_, SpeechError>>()?,
            hidden: embedding.cols,
            embedding,
            projector: self.floats("decoder.projector.weight")?,
            projector_bias: self.floats("decoder.projector.bias")?,
            joint: self.floats("joint.weight")?,
            joint_bias: self.floats("joint.bias")?,
            blank: self.number("blank")?,
            durations,
        })
    }
}
